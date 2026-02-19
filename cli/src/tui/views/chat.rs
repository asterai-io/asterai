use crate::tui::app::{
    AgentConfig, App, ChatMessage, ChatState, MessageRole, SLASH_COMMANDS, SPINNER_FRAMES, Screen,
    resolve_state_dir,
};
use crate::tui::ops;
use crossterm::event::{Event, KeyCode};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

const ROBOT: &[&str] = &[
    "     o     ",
    "     │     ",
    " ╔═══════╗ ",
    "═╣ ●   ● ╠═",
    " ║  ───  ║ ",
    " ╚═══════╝ ",
];

pub fn render(f: &mut Frame, state: &ChatState, app: &App) {
    let area = f.area();
    let agent = app.agent.as_ref();
    let bot_name = agent.map(|b| b.bot_name.as_str()).unwrap_or("Asterbot");
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(12),
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(area);
    render_banner(
        f,
        bot_name,
        agent,
        &state.banner_text,
        state.banner_loading,
        chunks[0],
    );
    let env_name = agent.map(|b| b.env_name.as_str()).unwrap_or("asterbot");
    render_messages(f, state, env_name, chunks[1]);
    let sep = Paragraph::new(Span::styled(
        "─".repeat(chunks[2].width as usize),
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(sep, chunks[2]);
    render_input(f, state, chunks[3]);
    // Slash menu renders above the separator, overlaying messages.
    let has_sub_menu = state.active_command.is_some() && !state.sub_matches.is_empty();
    let has_slash = (state.input.starts_with('/')
        && !state.input.contains(' ')
        && !state.slash_matches.is_empty())
        || has_sub_menu;
    let menu_count = match has_sub_menu {
        true => state.sub_matches.len(),
        false => state.slash_matches.len(),
    };
    let menu_h = match has_slash {
        true => menu_count.min(12) as u16,
        false => 0,
    };
    if menu_h > 0 {
        let menu_area = Rect::new(
            chunks[2].x,
            chunks[2].y.saturating_sub(menu_h),
            chunks[2].width,
            menu_h,
        );
        render_slash_menu(f, state, menu_area);
    }
}

pub async fn handle_event(app: &mut App, event: Event) -> eyre::Result<()> {
    // Handle paste events.
    if let Event::Paste(text) = &event {
        if let Screen::Chat(state) = &mut app.screen
            && !state.waiting
        {
            state.input.push_str(text);
            update_menus(state);
        }
        return Ok(());
    }
    let Event::Key(key_event) = event else {
        return Ok(());
    };
    // Only handle key press events (not release/repeat) to avoid duplication on Windows.
    if key_event.kind != crossterm::event::KeyEventKind::Press {
        return Ok(());
    }
    // Ignore Ctrl+key combos (e.g. Ctrl+V) to avoid stray characters.
    if key_event
        .modifiers
        .contains(crossterm::event::KeyModifiers::CONTROL)
    {
        return Ok(());
    }
    let code = key_event.code;
    let Screen::Chat(state) = &mut app.screen else {
        return Ok(());
    };
    if state.waiting {
        return Ok(());
    }
    let has_sub_menu = state.active_command.is_some() && !state.sub_matches.is_empty();
    let has_slash_menu = !state.slash_matches.is_empty() || has_sub_menu;
    match code {
        // --- Sub-menu navigation ---
        KeyCode::Up if has_sub_menu => {
            if state.sub_selected > 0 {
                state.sub_selected -= 1;
            }
        }
        KeyCode::Down if has_sub_menu => {
            if state.sub_selected + 1 < state.sub_matches.len() {
                state.sub_selected += 1;
            }
        }
        KeyCode::Enter | KeyCode::Tab if has_sub_menu => {
            let cmd_idx = state.active_command.unwrap();
            let sub_idx = state.sub_matches[state.sub_selected];
            let sub = &SLASH_COMMANDS[cmd_idx].subs[sub_idx];
            let cmd_name = SLASH_COMMANDS[cmd_idx].name;
            if sub.needs_arg {
                // Fill in command + subcommand, let user type the argument.
                state.input = format!("/{cmd_name} {} ", sub.name);
                state.active_command = None;
                state.sub_matches.clear();
                state.sub_selected = 0;
                state.slash_matches.clear();
                state.slash_selected = 0;
            } else {
                // Dispatch immediately (e.g. /tools list, /banner off).
                let full = format!("/{cmd_name} {}", sub.name);
                state.active_command = None;
                state.sub_matches.clear();
                state.sub_selected = 0;
                state.slash_matches.clear();
                state.slash_selected = 0;
                state.input.clear();
                dispatch_slash(app, &full).await?;
            }
        }
        KeyCode::Esc if has_sub_menu => {
            state.active_command = None;
            state.sub_matches.clear();
            state.sub_selected = 0;
            state.input.clear();
            state.slash_matches.clear();
            state.slash_selected = 0;
        }
        // --- Top-level slash menu navigation ---
        KeyCode::Up if has_slash_menu => {
            if state.slash_selected > 0 {
                state.slash_selected -= 1;
            }
        }
        KeyCode::Down if has_slash_menu => {
            if state.slash_selected + 1 < state.slash_matches.len() {
                state.slash_selected += 1;
            }
        }
        KeyCode::Enter | KeyCode::Tab
            if has_slash_menu && state.slash_selected < state.slash_matches.len() =>
        {
            let cmd_idx = state.slash_matches[state.slash_selected];
            let cmd = &SLASH_COMMANDS[cmd_idx];
            if !cmd.subs.is_empty() {
                // Has subcommands → enter sub-menu.
                state.input = format!("/{} ", cmd.name);
                state.active_command = Some(cmd_idx);
                state.sub_matches = (0..cmd.subs.len()).collect();
                state.sub_selected = 0;
                state.slash_matches.clear();
                state.slash_selected = 0;
            } else {
                // No subcommands → dispatch immediately.
                state.input = format!("/{}", cmd.name);
                state.slash_matches.clear();
                state.slash_selected = 0;
                let input = state.input.clone();
                state.input.clear();
                dispatch_slash(app, &input).await?;
            }
        }
        KeyCode::Esc if has_slash_menu => {
            state.slash_matches.clear();
            state.slash_selected = 0;
        }
        KeyCode::Up if !has_slash_menu => {
            if !state.input_history.is_empty() {
                let idx = match state.history_idx {
                    Some(i) if i > 0 => i - 1,
                    Some(i) => i,
                    None => state.input_history.len() - 1,
                };
                state.history_idx = Some(idx);
                state.input = state.input_history[idx].clone();
            }
        }
        KeyCode::Down if !has_slash_menu => {
            if let Some(idx) = state.history_idx {
                if idx + 1 < state.input_history.len() {
                    let new_idx = idx + 1;
                    state.history_idx = Some(new_idx);
                    state.input = state.input_history[new_idx].clone();
                } else {
                    state.history_idx = None;
                    state.input.clear();
                }
            }
        }
        KeyCode::Enter => {
            let input = state.input.trim().to_string();
            if input.is_empty() {
                return Ok(());
            }
            state.input_history.push(input.clone());
            state.history_idx = None;
            state.input.clear();
            state.slash_matches.clear();
            if input.starts_with('/') {
                dispatch_slash(app, &input).await?;
            } else {
                send_message(app, &input);
            }
        }
        KeyCode::Char(c) => {
            let Screen::Chat(state) = &mut app.screen else {
                return Ok(());
            };
            state.input.push(c);
            update_menus(state);
        }
        KeyCode::Backspace => {
            state.input.pop();
            update_menus(state);
        }
        KeyCode::Esc => {
            app.should_quit = true;
        }
        _ => {}
    }
    Ok(())
}

fn render_banner(
    f: &mut Frame,
    name: &str,
    agent: Option<&AgentConfig>,
    banner_text: &str,
    banner_loading: bool,
    area: Rect,
) {
    let model = agent.and_then(|b| b.model.as_deref()).unwrap_or("not set");
    let tools = agent.map(|b| &b.tools).cloned().unwrap_or_default();
    let tool_names: Vec<&str> = tools
        .iter()
        .map(|t| t.split(':').next_back().unwrap_or(t))
        .collect();
    let dirs_count = agent.map(|b| b.allowed_dirs.len()).unwrap_or(0);
    let title = format!(" {} ", name.to_uppercase());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(26), Constraint::Min(20)])
        .split(inner);
    // Vertical divider.
    let divider_x = cols[0].x + cols[0].width;
    for y in inner.y..inner.y + inner.height {
        if divider_x < area.x + area.width - 1 {
            let cell = ratatui::buffer::Cell::default()
                .set_char('│')
                .set_style(Style::default().fg(Color::Cyan))
                .clone();
            if let Some(c) = f
                .buffer_mut()
                .cell_mut(ratatui::layout::Position::new(divider_x, y))
            {
                *c = cell;
            }
        }
    }
    // Left column: greeting + robot.
    let greeting_name = agent
        .map(|a| a.user_name.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("USER");
    let mut left_lines: Vec<Line> = Vec::new();
    left_lines.push(
        Line::from(Span::styled(
            format!("GREETINGS, {}.", greeting_name.to_uppercase()),
            Style::default().fg(Color::Cyan).bold(),
        ))
        .alignment(Alignment::Center),
    );
    left_lines.push(
        Line::from(Span::styled(
            "SHALL WE PLAY A GAME?",
            Style::default().fg(Color::Cyan).bold(),
        ))
        .alignment(Alignment::Center),
    );
    left_lines.push(Line::from(""));
    for line in ROBOT {
        left_lines.push(
            Line::from(Span::styled(*line, Style::default().fg(Color::Cyan)))
                .alignment(Alignment::Center),
        );
    }
    f.render_widget(Paragraph::new(left_lines), cols[0]);
    // Right column: info.
    let right_area = Rect::new(
        cols[1].x + 1,
        cols[1].y,
        cols[1].width.saturating_sub(1),
        cols[1].height,
    );
    let tool_str = match tool_names.is_empty() {
        true => "none".to_string(),
        false => tool_names.join(" · "),
    };
    let mut right_lines: Vec<Line> = Vec::new();
    right_lines.push(Line::from(vec![
        Span::styled("MODEL   ", Style::default().fg(Color::DarkGray)),
        Span::styled(model, Style::default().fg(Color::White)),
    ]));
    // Wrap TOOLS across multiple lines if needed.
    let label_w = 8; // "TOOLS   ".len()
    let avail = right_area.width.saturating_sub(2) as usize;
    let tool_max = avail.saturating_sub(label_w);
    if tool_str.len() <= tool_max {
        right_lines.push(Line::from(vec![
            Span::styled("TOOLS   ", Style::default().fg(Color::DarkGray)),
            Span::styled(&tool_str, Style::default().fg(Color::White)),
        ]));
    } else {
        // Split tools into lines that fit.
        let mut first = true;
        let mut line_buf = String::new();
        for name in &tool_names {
            let sep = match line_buf.is_empty() {
                true => "",
                false => " · ",
            };
            if !line_buf.is_empty() && line_buf.len() + sep.len() + name.len() > tool_max {
                let prefix = match first {
                    true => "TOOLS   ",
                    false => "        ",
                };
                right_lines.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(Color::DarkGray)),
                    Span::styled(line_buf.clone(), Style::default().fg(Color::White)),
                ]));
                line_buf.clear();
                first = false;
            }
            if !line_buf.is_empty() {
                line_buf.push_str(" · ");
            }
            line_buf.push_str(name);
        }
        if !line_buf.is_empty() {
            let prefix = match first {
                true => "TOOLS   ",
                false => "        ",
            };
            right_lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::DarkGray)),
                Span::styled(line_buf, Style::default().fg(Color::White)),
            ]));
        }
    }
    if dirs_count > 0 {
        let dirs_label = match dirs_count {
            1 => "1 folder".to_string(),
            n => format!("{n} folders"),
        };
        right_lines.push(Line::from(vec![
            Span::styled("DIRS    ", Style::default().fg(Color::DarkGray)),
            Span::styled(dirs_label, Style::default().fg(Color::White)),
        ]));
    }
    right_lines.push(Line::from(""));
    right_lines.push(Line::from(vec![
        Span::styled("Type ", Style::default().fg(Color::DarkGray)),
        Span::styled("/", Style::default().fg(Color::Cyan)),
        Span::styled(" for commands · ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::styled(" to go back", Style::default().fg(Color::DarkGray)),
    ]));
    // Banner content (quote or tool data).
    if !banner_text.is_empty() {
        right_lines.push(Line::from(""));
        right_lines.push(Line::from(""));
        let display = match banner_loading {
            true => format!("{banner_text} ..."),
            false => banner_text.to_string(),
        };
        // Truncate to ~120 chars to avoid LLM duplication.
        let display = match display.len() > 120 {
            true => {
                let end = display
                    .char_indices()
                    .take_while(|(i, _)| *i <= 117)
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                format!("{}...", &display[..end])
            }
            false => display,
        };
        // Word-wrap to available width.
        let max_w = right_area.width.saturating_sub(2) as usize;
        for wrapped in textwrap(&display, max_w) {
            right_lines.push(Line::from(Span::styled(
                wrapped,
                Style::default().fg(Color::Rgb(100, 100, 120)).italic(),
            )));
        }
    }
    f.render_widget(Paragraph::new(right_lines), right_area);
}

fn render_messages(f: &mut Frame, state: &ChatState, env_name: &str, area: Rect) {
    if state.messages.is_empty() && !state.waiting {
        let text = Paragraph::new(Span::styled(
            "Send a message to start chatting.",
            Style::default().fg(Color::DarkGray),
        ))
        .alignment(Alignment::Center);
        let centered = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
        f.render_widget(text, centered);
        return;
    }
    let assistant_prefix = format!("{env_name}: ");
    let mut lines: Vec<Line> = Vec::new();
    for msg in &state.messages {
        let (prefix, style) = match msg.role {
            MessageRole::User => ("You: ", Style::default().fg(Color::Cyan)),
            MessageRole::Assistant => {
                (assistant_prefix.as_str(), Style::default().fg(Color::White))
            }
            MessageRole::System => ("", Style::default().fg(Color::Yellow)),
        };
        lines.push(Line::from(""));
        for text_line in msg.content.lines() {
            let is_first_line = text_line == msg.content.lines().next().unwrap_or("");
            let line_prefix = match is_first_line {
                true => prefix,
                false => "",
            };
            lines.push(Line::from(vec![
                Span::styled(line_prefix, style.bold()),
                Span::styled(text_line, style),
            ]));
        }
    }
    if state.waiting {
        let frame = SPINNER_FRAMES[state.spinner_tick % SPINNER_FRAMES.len()];
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(format!("{frame} "), Style::default().fg(Color::Cyan)),
            Span::styled("Thinking...", Style::default().fg(Color::DarkGray)),
        ]));
    }
    let total_lines = lines.len() as u16;
    let visible = area.height;
    let scroll = total_lines.saturating_sub(visible);
    let text = Paragraph::new(lines).scroll((scroll, 0));
    f.render_widget(text, area);
}

fn render_input(f: &mut Frame, state: &ChatState, area: Rect) {
    let input_area = Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(4),
        area.height,
    );
    let cursor = Span::styled("_", Style::default().fg(Color::DarkGray));
    let line = Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Cyan)),
        Span::raw(&state.input),
        cursor,
    ]);
    let input = Paragraph::new(line);
    f.render_widget(input, input_area);
}

fn render_slash_menu(f: &mut Frame, state: &ChatState, area: Rect) {
    let max_rows = area.height as usize;

    // Sub-menu mode: show sub-options for the active command.
    if let Some(cmd_idx) = state.active_command {
        let cmd = &SLASH_COMMANDS[cmd_idx];
        let skip = state
            .sub_selected
            .saturating_sub(max_rows.saturating_sub(1));
        let mut lines: Vec<Line> = Vec::new();
        for (i, &sub_idx) in state
            .sub_matches
            .iter()
            .enumerate()
            .skip(skip)
            .take(max_rows)
        {
            let sub = &cmd.subs[sub_idx];
            let is_selected = i == state.sub_selected;
            let pointer = match is_selected {
                true => "▸ ",
                false => "  ",
            };
            let name_style = match is_selected {
                true => Style::default().fg(Color::Cyan).bold(),
                false => Style::default().fg(Color::Cyan),
            };
            let desc_style = match is_selected {
                true => Style::default().fg(Color::White),
                false => Style::default().fg(Color::DarkGray),
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::raw(pointer),
                Span::styled(format!("{:<12}", sub.name), name_style),
                Span::styled(sub.description, desc_style),
            ]));
        }
        f.render_widget(Paragraph::new(lines), area);
        return;
    }

    // Top-level slash command menu.
    let skip = state
        .slash_selected
        .saturating_sub(max_rows.saturating_sub(1));
    let mut lines: Vec<Line> = Vec::new();
    for (i, &cmd_idx) in state
        .slash_matches
        .iter()
        .enumerate()
        .skip(skip)
        .take(max_rows)
    {
        let cmd = &SLASH_COMMANDS[cmd_idx];
        let is_selected = i == state.slash_selected;
        let pointer = match is_selected {
            true => "▸ ",
            false => "  ",
        };
        let name_style = match is_selected {
            true => Style::default().fg(Color::Cyan).bold(),
            false => Style::default().fg(Color::Cyan),
        };
        let desc_style = match is_selected {
            true => Style::default().fg(Color::White),
            false => Style::default().fg(Color::DarkGray),
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::raw(pointer),
            Span::styled(format!("/{:<12}", cmd.name), name_style),
            Span::styled(cmd.description, desc_style),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// Kick off an async banner content fetch if banner_mode is "auto".
pub fn start_banner_fetch(app: &mut App) {
    let Some(agent) = &app.agent else { return };
    if agent.banner_mode != "auto" {
        return;
    }
    if let Screen::Chat(state) = &mut app.screen {
        state.banner_loading = true;
    }
    let agent = agent.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.pending_banner = Some(rx);
    tokio::spawn(async move {
        let prompt = "In one brief line (under 80 chars), give a useful status update \
            using your available tools. Examples: current weather, top task, latest price. \
            No pleasantries, just the data.";
        let result =
            match tokio::task::spawn(async move { ops::call_converse(prompt, &agent).await }).await
            {
                Ok(r) => r.ok().flatten(),
                Err(_) => None,
            };
        let _ = tx.send(result);
    });
}

fn send_message(app: &mut App, input: &str) {
    let Screen::Chat(state) = &mut app.screen else {
        return;
    };
    state.messages.push(ChatMessage {
        role: MessageRole::User,
        content: input.to_string(),
    });
    state.waiting = true;
    let agent = app.agent.clone();
    let message = input.to_string();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.pending_response = Some(rx);
    tokio::spawn(async move {
        let result = match agent {
            Some(ref a) => {
                let a = a.clone();
                let msg = message.clone();
                match tokio::task::spawn(async move { ops::call_converse(&msg, &a).await }).await {
                    Ok(r) => r,
                    Err(e) => {
                        let panic_msg = if let Ok(s) = e.try_into_panic() {
                            s.downcast_ref::<String>()
                                .cloned()
                                .or_else(|| s.downcast_ref::<&str>().map(|s| s.to_string()))
                                .unwrap_or_else(|| "internal error".to_string())
                        } else {
                            "request was cancelled".to_string()
                        };
                        Err(eyre::eyre!("{panic_msg}"))
                    }
                }
            }
            None => Err(eyre::eyre!("no active agent")),
        };
        let _ = tx.send(result);
    });
}

async fn dispatch_slash(app: &mut App, input: &str) -> eyre::Result<()> {
    let parts: Vec<&str> = input[1..].split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }
    let cmd = parts[0].to_lowercase();
    let args = &parts[1..];
    match cmd.as_str() {
        "help" | "h" | "?" => cmd_help(app),
        "tools" | "t" => cmd_tools(app, args).await,
        "clear" | "c" => cmd_clear(app),
        "model" | "m" => cmd_model(app, args),
        "name" | "rename" => cmd_name(app, args),
        "username" | "whoami" => cmd_username(app, args),
        "dir" | "dirs" => cmd_dir(app, args),
        "status" | "info" => cmd_status(app),
        "banner" => cmd_banner(app, args),
        "push" => cmd_push(app).await,
        "pull" | "sync" => cmd_pull(app).await,
        "config" | "vars" => cmd_config(app, args).await,
        _ => {
            push_system(
                app,
                &format!("Unknown command: /{cmd}. Type /help for commands."),
            );
            Ok(())
        }
    }
}

fn update_menus(state: &mut ChatState) {
    // If we're in a sub-menu, filter sub-options.
    if let Some(cmd_idx) = state.active_command {
        let cmd = &SLASH_COMMANDS[cmd_idx];
        let prefix = format!("/{} ", cmd.name);
        if state.input.starts_with(&prefix) {
            let partial = state.input[prefix.len()..].to_lowercase();
            // If user typed past the sub-command (has a space after sub), close menu.
            if partial.contains(' ') {
                state.active_command = None;
                state.sub_matches.clear();
                state.sub_selected = 0;
            } else {
                state.sub_matches = cmd
                    .subs
                    .iter()
                    .enumerate()
                    .filter(|(_, sub)| sub.name.starts_with(partial.as_str()))
                    .map(|(i, _)| i)
                    .collect();
                state.sub_selected = state
                    .sub_selected
                    .min(state.sub_matches.len().saturating_sub(1));
            }
        } else {
            // Input no longer matches the command prefix - exit sub-menu.
            state.active_command = None;
            state.sub_matches.clear();
            state.sub_selected = 0;
            // Fall through to update top-level menu.
            update_top_level_menu(state);
        }
        return;
    }
    update_top_level_menu(state);
}

fn update_top_level_menu(state: &mut ChatState) {
    if state.input.starts_with('/') && !state.input.contains(' ') {
        let partial = &state.input[1..].to_lowercase();
        state.slash_matches = SLASH_COMMANDS
            .iter()
            .enumerate()
            .filter(|(_, cmd)| cmd.name.starts_with(partial.as_str()))
            .map(|(i, _)| i)
            .collect();
        state.slash_selected = state
            .slash_selected
            .min(state.slash_matches.len().saturating_sub(1));
    } else {
        state.slash_matches.clear();
        state.slash_selected = 0;
    }
}

fn push_system(app: &mut App, msg: &str) {
    if let Screen::Chat(state) = &mut app.screen {
        state.messages.push(ChatMessage {
            role: MessageRole::System,
            content: msg.to_string(),
        });
    }
}

fn cmd_help(app: &mut App) -> eyre::Result<()> {
    let mut lines = String::from("Commands:\n");
    for cmd in SLASH_COMMANDS {
        lines.push_str(&format!("  /{:<12} {}\n", cmd.name, cmd.description));
    }
    push_system(app, &lines);
    Ok(())
}

async fn cmd_tools(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    let tools = app
        .agent
        .as_ref()
        .map(|b| &b.tools)
        .cloned()
        .unwrap_or_default();
    if args.is_empty() {
        let mut msg = String::from("Enabled tools:\n");
        if tools.is_empty() {
            msg.push_str("  (none)");
        } else {
            for tool in &tools {
                msg.push_str(&format!("  {tool}\n"));
            }
        }
        msg.push_str(
            "\n/tools add <namespace:component>  Add a tool\n\
             /tools remove <component>  Remove a tool",
        );
        push_system(app, &msg);
        return Ok(());
    }
    let Some(agent) = &app.agent else {
        push_system(app, "No active agent.");
        return Ok(());
    };
    let env_name = agent.env_name.clone();
    if args[0] == "add" && args.len() > 1 {
        // Auto-prepend user namespace if missing (e.g. "trello" → "seadog:trello").
        let component = match args[1].contains(':') {
            true => args[1].to_string(),
            false => {
                let ns = crate::auth::Auth::read_user_or_fallback_namespace();
                format!("{ns}:{}", args[1])
            }
        };
        match ops::add_component(&env_name, &component).await {
            Ok(_) => {
                if let Some(agent) = &mut app.agent {
                    let base = component.split('@').next().unwrap_or(&component);
                    if !agent.tools.contains(&base.to_string()) {
                        agent.tools.push(base.to_string());
                    }
                }
                save_tools(app);
                push_system(app, &format!("+ {component}"));
            }
            Err(e) => push_system(app, &format!("Failed to add: {e:#}")),
        }
        return Ok(());
    }
    if (args[0] == "remove" || args[0] == "rm") && args.len() > 1 {
        let component = args[1];
        match ops::remove_component(&env_name, component).await {
            Ok(_) => {
                let base = component.split('@').next().unwrap_or(component);
                if let Some(agent) = &mut app.agent {
                    agent.tools.retain(|t| t != base);
                }
                save_tools(app);
                push_system(app, &format!("- {component}"));
            }
            Err(e) => push_system(app, &format!("Failed to remove: {e:#}")),
        }
        return Ok(());
    }
    push_system(app, "Usage: /tools [add|remove] <namespace:component>");
    Ok(())
}

fn cmd_clear(app: &mut App) -> eyre::Result<()> {
    if let Screen::Chat(state) = &mut app.screen {
        state.messages.clear();
    }
    if let Some(agent) = &app.agent {
        let state_dir = resolve_state_dir(&agent.env_name);
        let conv_path = state_dir.join("conversation.json");
        let _ = std::fs::write(conv_path, "[]");
    }
    push_system(app, "Conversation history cleared.");
    Ok(())
}

fn cmd_model(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    if args.is_empty() {
        let model = app
            .agent
            .as_ref()
            .and_then(|b| b.model.as_deref())
            .unwrap_or("(not set)");
        push_system(
            app,
            &format!("Current model: {model}\n\nSwitch with: /model <provider/model>"),
        );
        return Ok(());
    }
    let new_model = args[0].to_string();
    if let Some(agent) = &app.agent {
        let _ = ops::set_var(&agent.env_name, "ASTERBOT_MODEL", &new_model);
    }
    if let Some(agent) = &mut app.agent {
        agent.model = Some(new_model.clone());
    }
    push_system(app, &format!("Model set to {new_model}"));
    Ok(())
}

fn cmd_name(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    if args.is_empty() {
        let name = app
            .agent
            .as_ref()
            .map(|b| b.bot_name.as_str())
            .unwrap_or("Asterbot");
        push_system(
            app,
            &format!("Agent name: {name}\n\nChange with: /name <new name>"),
        );
        return Ok(());
    }
    let new_name = args.join(" ");
    if let Some(agent) = &app.agent {
        let _ = ops::set_var(&agent.env_name, "ASTERBOT_BOT_NAME", &new_name);
    }
    if let Some(agent) = &mut app.agent {
        agent.bot_name = new_name.clone();
    }
    push_system(app, &format!("Agent renamed to {new_name}"));
    Ok(())
}

fn cmd_username(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    if args.is_empty() {
        let name = app
            .agent
            .as_ref()
            .map(|b| b.user_name.as_str())
            .unwrap_or("not set");
        push_system(
            app,
            &format!("Your display name: {name}\n\nChange with: /username <name>"),
        );
        return Ok(());
    }
    let new_name = args.join(" ");
    if let Some(agent) = &app.agent {
        let _ = ops::set_var(&agent.env_name, "ASTERBOT_USER_NAME", &new_name);
    }
    if let Some(agent) = &mut app.agent {
        agent.user_name = new_name.clone();
    }
    push_system(app, &format!("Display name set to {new_name}"));
    Ok(())
}

fn cmd_banner(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    if args.is_empty() {
        let mode = app
            .agent
            .as_ref()
            .map(|a| a.banner_mode.as_str())
            .unwrap_or("auto");
        push_system(
            app,
            &format!(
                "Banner mode: {mode}\n\n\
                 /banner auto  - agent picks content from tools\n\
                 /banner quote - random quotes only\n\
                 /banner off   - no banner content"
            ),
        );
        return Ok(());
    }
    let mode = args[0].to_lowercase();
    if !["auto", "quote", "off"].contains(&mode.as_str()) {
        push_system(app, "Invalid mode. Use: auto, quote, or off");
        return Ok(());
    }
    if let Some(agent) = &app.agent {
        let _ = ops::set_var(&agent.env_name, "ASTERBOT_BANNER", &mode);
    }
    if let Some(agent) = &mut app.agent {
        agent.banner_mode = mode.clone();
    }
    match mode.as_str() {
        "auto" => {
            push_system(app, "Banner set to auto (fetching from tools).");
            start_banner_fetch(app);
        }
        "quote" => {
            if let Screen::Chat(state) = &mut app.screen {
                state.banner_text = crate::tui::app::random_quote().to_string();
                state.banner_loading = false;
            }
            push_system(app, "Banner set to random quotes.");
        }
        "off" => {
            if let Screen::Chat(state) = &mut app.screen {
                state.banner_text.clear();
                state.banner_loading = false;
            }
            push_system(app, "Banner content disabled.");
        }
        _ => {}
    }
    Ok(())
}

fn cmd_dir(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    let dirs = app
        .agent
        .as_ref()
        .map(|b| b.allowed_dirs.clone())
        .unwrap_or_default();
    if args.is_empty() || args[0] == "list" {
        let mut msg = String::from("Allowed directories:\n");
        if dirs.is_empty() {
            msg.push_str("  (none)\n");
        } else {
            for dir in &dirs {
                msg.push_str(&format!("  {dir}\n"));
            }
        }
        msg.push_str("\n/dir add <path>     Grant access\n/dir remove <path>  Revoke access");
        push_system(app, &msg);
        return Ok(());
    }
    if args[0] == "add" && args.len() > 1 {
        let path = args[1..].join(" ");
        let resolved = std::path::Path::new(&path)
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(path);
        if let Some(agent) = &mut app.agent {
            agent.allowed_dirs.push(resolved.clone());
        }
        save_allowed_dirs(app);
        push_system(app, &format!("+ {resolved}"));
        return Ok(());
    }
    if (args[0] == "remove" || args[0] == "rm") && args.len() > 1 {
        let path = args[1..].join(" ");
        let resolved = std::path::Path::new(&path)
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(path);
        if let Some(agent) = &mut app.agent {
            agent.allowed_dirs.retain(|d| d != &resolved);
        }
        save_allowed_dirs(app);
        push_system(app, &format!("- {resolved}"));
        return Ok(());
    }
    push_system(app, "Usage: /dir [list|add|remove] <path>");
    Ok(())
}

fn cmd_status(app: &mut App) -> eyre::Result<()> {
    let Some(agent) = &app.agent else {
        push_system(app, "No active agent.");
        return Ok(());
    };
    let mut msg = format!(
        "Agent Status:\n\
         \x20 Name:        {}\n\
         \x20 Environment: {}\n\
         \x20 Model:       {}\n\
         \x20 Provider:    {}",
        agent.bot_name,
        agent.env_name,
        agent.model.as_deref().unwrap_or("(not set)"),
        agent.provider,
    );
    if !agent.tools.is_empty() {
        let tool_names: Vec<&str> = agent
            .tools
            .iter()
            .map(|t| t.split(':').next_back().unwrap_or(t))
            .collect();
        msg.push_str(&format!("\n  Tools:       {}", tool_names.join(", ")));
    }
    if !agent.allowed_dirs.is_empty() {
        msg.push_str(&format!(
            "\n  Directories: {} folder(s)",
            agent.allowed_dirs.len()
        ));
    }
    push_system(app, &msg);
    Ok(())
}

async fn cmd_push(app: &mut App) -> eyre::Result<()> {
    let Some(agent) = &app.agent else {
        push_system(app, "No active agent.");
        return Ok(());
    };
    let env_name = agent.env_name.clone();
    push_system(app, &format!("Pushing {env_name}..."));
    match ops::push_env(&env_name).await {
        Ok(_) => push_system(app, &format!("Pushed {env_name} to cloud.")),
        Err(e) => push_system(app, &format!("Push failed: {e:#}")),
    }
    Ok(())
}

async fn cmd_pull(app: &mut App) -> eyre::Result<()> {
    let Some(agent) = &app.agent else {
        push_system(app, "No active agent.");
        return Ok(());
    };
    let env_name = agent.env_name.clone();
    push_system(app, &format!("Pulling {env_name}..."));
    match ops::pull_env(&env_name).await {
        Ok(_) => push_system(app, &format!("Pulled {env_name} from cloud.")),
        Err(e) => push_system(app, &format!("Pull failed: {e:#}")),
    }
    Ok(())
}

async fn cmd_config(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    if args.is_empty() || args[0] == "list" {
        let Some(agent) = &app.agent else {
            push_system(app, "No active agent.");
            return Ok(());
        };
        let env_name = agent.env_name.clone();
        let data = ops::inspect_environment(&env_name).await?;
        let vars = data.map(|d| d.vars).unwrap_or_default();
        let mut msg = format!("Environment variables ({env_name}):\n");
        if vars.is_empty() {
            msg.push_str("  (none)");
        } else {
            for v in &vars {
                msg.push_str(&format!("  {v}\n"));
            }
        }
        msg.push_str("\n/config set KEY=VALUE  Set a variable");
        push_system(app, &msg);
        return Ok(());
    }
    if args[0] == "set" && args.len() > 1 {
        let expr = args[1..].join(" ");
        let Some((key, value)) = expr.split_once('=') else {
            push_system(app, "Usage: /config set KEY=VALUE");
            return Ok(());
        };
        let key = key.trim();
        let value = value.trim();
        let Some(agent) = &app.agent else {
            push_system(app, "No active agent.");
            return Ok(());
        };
        let env_name = agent.env_name.clone();
        match ops::set_var(&env_name, key, value) {
            Ok(_) => push_system(app, &format!("{key} = {value}")),
            Err(e) => push_system(app, &format!("Failed: {e:#}")),
        }
        return Ok(());
    }
    push_system(app, "Usage: /config [list|set KEY=VALUE]");
    Ok(())
}

fn save_tools(app: &App) {
    let Some(agent) = &app.agent else { return };
    let value = agent.tools.join(",");
    let _ = ops::set_var(&agent.env_name, "ASTERBOT_TOOLS", &value);
}

fn save_allowed_dirs(app: &App) {
    let Some(agent) = &app.agent else { return };
    let value = agent.allowed_dirs.join(",");
    let _ = ops::set_var(&agent.env_name, "ASTERBOT_ALLOWED_DIRS", &value);
}

/// Simple word-wrap for banner text.
fn textwrap(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.len() + 1 + word.len() <= max_width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
