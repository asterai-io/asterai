use crate::tui::Tty;
use crate::tui::app::{
    AgentConfig, App, CORE_COMPONENTS, ChatState, DEFAULT_TOOLS, PROVIDERS, Screen, SetupState,
    SetupStep, default_user_name, resolve_state_dir, sanitize_bot_name,
};
use crate::tui::ops;
use crossterm::event::{Event, KeyCode};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn render(f: &mut Frame, state: &SetupState) {
    let area = f.area();
    let block = Block::default()
        .title(" Agent Setup ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let content_area = Rect::new(
        inner.x + 2,
        inner.y + 1,
        inner.width.saturating_sub(4),
        inner.height.saturating_sub(2),
    );
    match &state.step {
        SetupStep::Name => render_name_step(f, state, content_area),
        SetupStep::Username => render_username_step(f, state, content_area),
        SetupStep::Provider => render_provider_step(f, state, content_area),
        SetupStep::ApiKey => render_api_key_step(f, state, content_area),
        SetupStep::Model => render_model_step(f, state, content_area),
        SetupStep::Directories => {}
        SetupStep::Provisioning {
            current,
            total,
            message,
        } => {
            render_provisioning(f, *current, *total, message, content_area);
        }
        SetupStep::WarmUp => {
            let text = Paragraph::new("Warming up (first-time compilation may take a moment)...")
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(text, content_area);
        }
        SetupStep::PushPrompt => render_push_prompt(f, content_area),
    }
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

    // Esc navigates back through setup steps, or returns to picker from first step.
    if code == KeyCode::Esc {
        let Screen::Setup(state) = &mut app.screen else {
            return Ok(());
        };
        match &state.step {
            SetupStep::Name => {
                app.screen = Screen::Picker(crate::tui::app::PickerState::loading(0));
                return Ok(());
            }
            SetupStep::Username => {
                state.step = SetupStep::Name;
                state.input = state.bot_name.clone();
            }
            SetupStep::Provider => {
                state.step = SetupStep::Username;
                state.input = state.user_name.clone();
            }
            SetupStep::ApiKey => {
                state.step = SetupStep::Provider;
                state.input.clear();
                state.error = None;
            }
            SetupStep::Model => {
                state.step = SetupStep::ApiKey;
                state.input = state.api_key.clone();
            }
            // Non-interactive steps: ignore Esc.
            _ => {}
        }
        return Ok(());
    }

    let Screen::Setup(state) = &mut app.screen else {
        return Ok(());
    };
    match &state.step {
        SetupStep::Name => handle_name(state, code),
        SetupStep::Username => handle_username(state, code),
        SetupStep::Provider => handle_provider(state, code),
        SetupStep::ApiKey => handle_api_key(state, code),
        SetupStep::Model => handle_model(state, code),
        SetupStep::Directories => {}
        SetupStep::Provisioning { .. } => {}
        SetupStep::WarmUp => {}
        SetupStep::PushPrompt => {
            handle_push_prompt(app, code, terminal).await?;
            return Ok(());
        }
    }
    let Screen::Setup(state) = &mut app.screen else {
        return Ok(());
    };
    if matches!(state.step, SetupStep::Provisioning { .. }) {
        run_provisioning(app, terminal).await?;
    }
    Ok(())
}

fn render_name_step(f: &mut Frame, state: &SetupState, area: Rect) {
    let mut lines = vec![
        Line::from(Span::styled("Name your agent", Style::default().bold())),
        Line::from(""),
    ];
    if let Some(err) = &state.error {
        lines.push(Line::from(Span::styled(
            err.as_str(),
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(vec![
        Span::raw("Name (default: Asterbot): "),
        Span::styled(&state.input, Style::default().fg(Color::Cyan)),
        Span::styled("_", Style::default().fg(Color::DarkGray)),
    ]));
    if !state.input.is_empty() {
        let sanitized = sanitize_bot_name(&state.input);
        lines.push(Line::from(Span::styled(
            format!("(environment: {sanitized})"),
            Style::default().fg(Color::DarkGray),
        )));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn render_username_step(f: &mut Frame, state: &SetupState, area: Rect) {
    let default = default_user_name();
    let mut lines = vec![
        Line::from(Span::styled(
            "What should the agent call you?",
            Style::default().bold(),
        )),
        Line::from(""),
    ];
    lines.push(Line::from(vec![
        Span::raw(format!("Your name (default: {default}): ")),
        Span::styled(&state.input, Style::default().fg(Color::Cyan)),
        Span::styled("_", Style::default().fg(Color::DarkGray)),
    ]));
    f.render_widget(Paragraph::new(lines), area);
}

fn render_provider_step(f: &mut Frame, state: &SetupState, area: Rect) {
    let mut lines = vec![
        Line::from(Span::styled("Which LLM provider?", Style::default().bold())),
        Line::from(""),
    ];
    let all_items: Vec<(&str, bool)> = PROVIDERS
        .iter()
        .map(|(name, _, _)| (*name, false))
        .chain(std::iter::once(("asterai managed LLM (coming soon)", true)))
        .collect();
    for (i, (name, disabled)) in all_items.iter().enumerate() {
        let is_selected = i == state.provider_idx;
        let pointer = match is_selected {
            true => "▸ ",
            false => "  ",
        };
        lines.push(Line::from(vec![
            Span::raw(pointer),
            Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::DarkGray)),
            Span::styled(
                *name,
                if *disabled {
                    if is_selected {
                        Style::default().fg(Color::Rgb(255, 160, 50)).bold()
                    } else {
                        Style::default().fg(Color::Rgb(255, 160, 50))
                    }
                } else if is_selected {
                    Style::default().fg(Color::Cyan).bold()
                } else {
                    Style::default()
                },
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑↓ navigate · enter select · esc back",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(Paragraph::new(lines), area);
}

fn render_api_key_step(f: &mut Frame, state: &SetupState, area: Rect) {
    let provider_name = PROVIDERS
        .get(state.provider_idx)
        .map(|p| p.0)
        .unwrap_or("LLM");
    let mut lines = vec![
        Line::from(Span::styled(
            format!("Enter your {provider_name} API key"),
            Style::default().bold(),
        )),
        Line::from(""),
    ];
    if let Some(err) = &state.error {
        lines.push(Line::from(Span::styled(
            err.as_str(),
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
    }
    let masked: String = "*".repeat(state.input.len());
    lines.push(Line::from(vec![
        Span::raw("API key: "),
        Span::styled(masked, Style::default().fg(Color::Yellow)),
        Span::styled("_", Style::default().fg(Color::DarkGray)),
    ]));
    f.render_widget(Paragraph::new(lines), area);
}

fn render_model_step(f: &mut Frame, state: &SetupState, area: Rect) {
    let models = PROVIDERS
        .get(state.provider_idx)
        .map(|p| p.2)
        .unwrap_or(&[]);
    let mut lines = vec![
        Line::from(Span::styled("Select model", Style::default().bold())),
        Line::from(""),
    ];
    for (i, (_, label)) in models.iter().enumerate() {
        let is_selected = i == state.model_idx;
        let pointer = match is_selected {
            true => "▸ ",
            false => "  ",
        };
        lines.push(Line::from(vec![
            Span::raw(pointer),
            Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::DarkGray)),
            Span::styled(
                *label,
                match is_selected {
                    true => Style::default().fg(Color::Cyan).bold(),
                    false => Style::default(),
                },
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑↓ navigate · enter select · esc back",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(Paragraph::new(lines), area);
}

fn render_provisioning(f: &mut Frame, current: usize, total: usize, message: &str, area: Rect) {
    let mut lines = vec![
        Line::from(Span::styled("Setting up agent...", Style::default().bold())),
        Line::from(""),
        Line::from(format!("[{current}/{total}] {message}")),
    ];
    let bar_width = area.width.saturating_sub(4) as usize;
    let filled = match total {
        0 => 0,
        _ => (current * bar_width) / total,
    };
    let empty = bar_width.saturating_sub(filled);
    lines.push(Line::from(Span::styled(
        format!("[{}{}]", "█".repeat(filled), "░".repeat(empty)),
        Style::default().fg(Color::Cyan),
    )));
    f.render_widget(Paragraph::new(lines), area);
}

fn render_push_prompt(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled("Push to cloud?", Style::default().bold())),
        Line::from(Span::styled(
            "Pushing saves your agent to asterai so you can access it from anywhere.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from("Press Y to push, N to skip."),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn handle_name(state: &mut SetupState, code: KeyCode) {
    match code {
        KeyCode::Char(c) => {
            state.input.push(c);
            state.error = None;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Enter => {
            let name = match state.input.trim().is_empty() {
                true => "Asterbot".to_string(),
                false => state.input.trim().to_string(),
            };
            state.bot_name = name;
            state.env_name = sanitize_bot_name(&state.bot_name);
            state.input.clear();
            state.step = SetupStep::Username;
        }
        KeyCode::Esc => { /* handled above */ }
        _ => {}
    }
}

fn handle_username(state: &mut SetupState, code: KeyCode) {
    match code {
        KeyCode::Char(c) => {
            state.input.push(c);
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Enter => {
            let name = match state.input.trim().is_empty() {
                true => default_user_name(),
                false => state.input.trim().to_string(),
            };
            state.user_name = name;
            state.input.clear();
            state.step = SetupStep::Provider;
        }
        _ => {}
    }
}

fn handle_provider(state: &mut SetupState, code: KeyCode) {
    let total = PROVIDERS.len() + 1; // +1 for "coming soon" entry
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            if state.provider_idx > 0 {
                state.provider_idx -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.provider_idx + 1 < total {
                state.provider_idx += 1;
            }
        }
        KeyCode::Enter => {
            if state.provider_idx < PROVIDERS.len() {
                state.step = SetupStep::ApiKey;
                state.input.clear();
            }
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            let num = c.to_digit(10).unwrap() as usize;
            if num >= 1 && num <= PROVIDERS.len() {
                state.provider_idx = num - 1;
                state.step = SetupStep::ApiKey;
                state.input.clear();
            }
        }
        _ => {}
    }
}

fn handle_api_key(state: &mut SetupState, code: KeyCode) {
    match code {
        KeyCode::Char(c) => {
            state.input.push(c);
            state.error = None;
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Enter => {
            let key = state.input.trim().to_string();
            if key.is_empty() {
                state.error = Some("API key is required.".to_string());
                return;
            }
            state.api_key = key;
            state.input.clear();
            state.model_idx = 0;
            state.step = SetupStep::Model;
        }
        _ => {}
    }
}

fn handle_model(state: &mut SetupState, code: KeyCode) {
    let models = PROVIDERS
        .get(state.provider_idx)
        .map(|p| p.2)
        .unwrap_or(&[]);
    let total = models.len();
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            if state.model_idx > 0 {
                state.model_idx -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.model_idx + 1 < total {
                state.model_idx += 1;
            }
        }
        KeyCode::Enter => {
            if let Some((model_id, _)) = models.get(state.model_idx) {
                state.model = model_id.to_string();
            }
            state.input.clear();
            state.step = SetupStep::Provisioning {
                current: 0,
                total: 0,
                message: "Starting...".to_string(),
            };
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            let num = c.to_digit(10).unwrap() as usize;
            if num >= 1 && num <= total {
                state.model_idx = num - 1;
                if let Some((model_id, _)) = models.get(state.model_idx) {
                    state.model = model_id.to_string();
                }
                state.input.clear();
                state.step = SetupStep::Provisioning {
                    current: 0,
                    total: 0,
                    message: "Starting...".to_string(),
                };
            }
        }
        _ => {}
    }
}

async fn handle_push_prompt(
    app: &mut App,
    code: KeyCode,
    _terminal: &mut Terminal<CrosstermBackend<Tty>>,
) -> eyre::Result<()> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            let env_name = app
                .agent
                .as_ref()
                .map(|b| b.env_name.clone())
                .unwrap_or_default();
            let _ = ops::push_env(&env_name).await;
            app.screen = Screen::Chat(ChatState::default());
            super::chat::start_banner_fetch(app);
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.screen = Screen::Chat(ChatState::default());
            super::chat::start_banner_fetch(app);
        }
        _ => {}
    }
    Ok(())
}

async fn run_provisioning(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<Tty>>,
) -> eyre::Result<()> {
    let Screen::Setup(state) = &app.screen else {
        return Ok(());
    };
    let env_name = state.env_name.clone();
    let bot_name = state.bot_name.clone();
    let user_name = state.user_name.clone();
    let provider_idx = state.provider_idx;
    let api_key = state.api_key.clone();
    let model = state.model.clone();
    let state_dir = resolve_state_dir(&state.env_name);
    let _ = std::fs::create_dir_all(&state_dir);
    let allowed_dirs = vec![state_dir.to_string_lossy().to_string()];
    let all_components: Vec<&str> = CORE_COMPONENTS
        .iter()
        .chain(DEFAULT_TOOLS.iter())
        .copied()
        .collect();
    // Components + init + 7 vars.
    let total = all_components.len() + 8;
    let mut current = 0;
    update_provisioning(app, current, total, "Creating environment...");
    terminal.draw(|f| super::render(f, app))?;
    match ops::env_init(&env_name) {
        Ok(_) => {}
        Err(e) => {
            let msg = format!("{e:#}");
            if !msg.contains("already exists") {
                return Err(e);
            }
            let _ = ops::pull_env(&env_name).await;
        }
    }
    current += 1;
    for comp in &all_components {
        update_provisioning(app, current, total, &format!("Adding {comp}..."));
        terminal.draw(|f| super::render(f, app))?;
        match ops::add_component(&env_name, comp).await {
            Ok(_) => {}
            Err(e) => {
                let msg = format!("{e:#}");
                if !msg.contains("already") {
                    return Err(e);
                }
            }
        }
        current += 1;
    }
    let provider = PROVIDERS.get(provider_idx);
    let env_var = provider.map(|p| p.1).unwrap_or("API_KEY");
    let wasi_state_dir = state_dir.to_string_lossy().replace('\\', "/");
    let tool_names: String = DEFAULT_TOOLS.join(",");
    let dirs_value = allowed_dirs.join(",");
    let vars = vec![
        ("ASTERBOT_MODEL", model.as_str()),
        (env_var, api_key.as_str()),
        ("ASTERBOT_TOOLS", &tool_names),
        ("ASTERBOT_HOST_DIR", &wasi_state_dir),
        ("ASTERBOT_BOT_NAME", bot_name.as_str()),
        ("ASTERBOT_USER_NAME", user_name.as_str()),
        ("ASTERBOT_ALLOWED_DIRS", dirs_value.as_str()),
    ];
    for (key, value) in &vars {
        update_provisioning(app, current, total, &format!("Setting {key}..."));
        terminal.draw(|f| super::render(f, app))?;
        let _ = ops::set_var(&env_name, key, value);
        current += 1;
    }
    // Build agent config.
    let namespace = crate::auth::Auth::read_user_or_fallback_namespace();
    let agent = AgentConfig {
        env_name: env_name.clone(),
        namespace,
        bot_name,
        user_name,
        model: Some(model),
        provider: provider.map(|p| p.0).unwrap_or("custom").to_string(),
        tools: DEFAULT_TOOLS.iter().map(|s| s.to_string()).collect(),
        allowed_dirs,
        banner_mode: "auto".to_string(),
        preferred_port: None,
    };
    app.agent = Some(agent);
    let Screen::Setup(state) = &mut app.screen else {
        return Ok(());
    };
    state.step = SetupStep::WarmUp;
    terminal.draw(|f| super::render(f, app))?;
    if let Some(agent) = &app.agent {
        let _ = ops::call_converse("hello", agent).await;
    }
    let Screen::Setup(state) = &mut app.screen else {
        return Ok(());
    };
    state.step = SetupStep::PushPrompt;
    Ok(())
}

fn update_provisioning(app: &mut App, current: usize, total: usize, message: &str) {
    if let Screen::Setup(state) = &mut app.screen {
        state.step = SetupStep::Provisioning {
            current,
            total,
            message: message.to_string(),
        };
    }
}
