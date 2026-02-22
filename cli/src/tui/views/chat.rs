use crate::tui::app::{
    App, ChatMessage, ChatState, CORE_COMPONENTS, DynamicItem, MessageRole,
    PROVIDERS, PickerState, SLASH_COMMANDS, SPINNER_FRAMES, Screen, required_env_vars,
    resolve_state_dir,
};
use crate::tui::ops;
use crossterm::event::{Event, KeyCode};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::time::{Duration, Instant};

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
    // Compute input box height based on visual wrap lines.
    // Inner width = total width - 2 (left/right borders).
    let inner_w = area.width.saturating_sub(2) as usize;
    let input_text = if state.has_env_prompt() {
        &state.env_prompt_input
    } else {
        &state.input
    };
    let prefix_len = if state.has_env_prompt() {
        state.env_prompt_vars.get(state.env_prompt_idx)
            .map(|v| v.len() + 1) // "VAR="
            .unwrap_or(0)
    } else {
        2 // "> "
    };
    let content_len = prefix_len + input_text.len() + 1; // +1 for cursor
    let visual_lines = if inner_w > 0 {
        ((content_len + inner_w - 1) / inner_w).max(1) as u16
    } else {
        1
    };
    let input_h = (visual_lines + 2).max(3); // +2 for top/bottom borders
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(12),
            Constraint::Min(4),
            Constraint::Length(input_h),
        ])
        .split(area);
    render_banner(f, bot_name, state, app, chunks[0]);
    let env_name = agent.map(|b| b.env_name.as_str()).unwrap_or("asterbot");
    render_messages(f, state, env_name, chunks[1]);
    // Input box with border (top border acts as separator).
    let input_area = chunks[2];
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let input_inner = input_block.inner(input_area);
    f.render_widget(input_block, input_area);
    render_input(f, state, input_inner);
    // Toast notification rendered on the banner bottom-right.
    if let Some(toast) = &state.toast {
        let banner_area = chunks[0];
        let msg = format!(" {} ", toast);
        let msg_w = msg.len() as u16;
        if msg_w + 2 < banner_area.width {
            let toast_area = Rect::new(
                banner_area.x + banner_area.width.saturating_sub(msg_w + 1),
                banner_area.y + banner_area.height.saturating_sub(1),
                msg_w,
                1,
            );
            f.render_widget(
                Paragraph::new(Span::styled(msg, Style::default().fg(state.toast_color))),
                toast_area,
            );
        }
    }
    // Slash menu renders above the input box, overlaying messages.
    let has_dynamic = state.dynamic_command.is_some();
    let has_sub_menu = state.active_command.is_some() && !state.sub_matches.is_empty();
    let has_slash = (state.input.starts_with('/') && !state.input.contains(' ') && !state.slash_matches.is_empty())
        || has_sub_menu
        || has_dynamic;
    let menu_count = if has_dynamic {
        if state.dynamic_loading { 1 } else { state.dynamic_matches.len() }
    } else if has_sub_menu {
        state.sub_matches.len()
    } else {
        state.slash_matches.len()
    };
    let content_h = match has_slash {
        true => menu_count as u16,
        false => 0,
    };
    if content_h > 0 {
        // +2 for top/bottom borders, capped to available message area.
        let max_menu = chunks[1].height;
        let menu_h = (content_h + 2).min(max_menu);
        let menu_area = Rect::new(
            input_area.x + 1,
            input_area.y.saturating_sub(menu_h),
            input_area.width.saturating_sub(2),
            menu_h,
        );
        f.render_widget(Clear, menu_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(menu_area);
        f.render_widget(block, menu_area);
        render_slash_menu(f, state, inner);
    }
    // Info overlay renders over the message area.
    if state.info_overlay.is_some() {
        render_info_overlay(f, state, chunks[1]);
    }
}

pub async fn handle_event(app: &mut App, event: Event) -> eyre::Result<()> {
    // Handle paste events.
    if let Event::Paste(text) = &event {
        if let Screen::Chat(state) = &mut app.screen {
            if !state.waiting {
                state.input.push_str(text);
                update_menus(state);
            }
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
    if key_event.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
        return Ok(());
    }
    let code = key_event.code;
    // Pre-capture agent tools for remove picker (avoids borrow conflict with screen).
    let agent_tools: Vec<String> = app
        .agent
        .as_ref()
        .map(|a| a.tools.clone())
        .unwrap_or_default();
    let Screen::Chat(state) = &mut app.screen else {
        return Ok(());
    };
    if state.waiting {
        return Ok(());
    }
    // --- Info overlay handling (scroll or dismiss) ---
    if state.info_overlay.is_some() {
        match code {
            KeyCode::Up => {
                state.info_overlay_scroll = state.info_overlay_scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                if let Some(lines) = &state.info_overlay {
                    let max = (lines.len() as u16).saturating_sub(1);
                    state.info_overlay_scroll = (state.info_overlay_scroll + 1).min(max);
                }
            }
            KeyCode::PageUp => {
                state.info_overlay_scroll = state.info_overlay_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                if let Some(lines) = &state.info_overlay {
                    let max = (lines.len() as u16).saturating_sub(1);
                    state.info_overlay_scroll = (state.info_overlay_scroll + 10).min(max);
                }
            }
            _ => {
                state.info_overlay = None;
                state.info_overlay_scroll = 0;
            }
        }
        return Ok(());
    }
    let has_env_prompt = state.has_env_prompt();
    let has_dynamic = state.dynamic_command.is_some();
    let has_sub_menu = state.active_command.is_some() && !state.sub_matches.is_empty();
    let has_slash_menu = !state.slash_matches.is_empty() || has_sub_menu;
    match code {
        // --- Env var prompt handling ---
        KeyCode::Enter if has_env_prompt => {
            let value = state.env_prompt_input.trim().to_string();
            let var_name = state.env_prompt_vars[state.env_prompt_idx].clone();
            if !value.is_empty() {
                let mut ok = false;
                if let Some(agent) = &app.agent {
                    let env_name = agent.env_name.clone();
                    match ops::set_var(&env_name, &var_name, &value) {
                        Ok(_) => {
                            set_toast_color(app, &format!("{var_name} set."), Color::Green);
                            ok = true;
                        }
                        Err(e) => {
                            push_system(app, &format!("Failed to set {var_name}: {e:#}"));
                            set_toast_color(app, &format!("{var_name} FAILED"), Color::Red);
                        }
                    }
                }
                if !ok && app.agent.is_none() {
                    set_toast(app, &format!("{var_name} skipped (no agent)."));
                }
            } else {
                set_toast(app, &format!("{var_name} skipped."));
            }
            if let Screen::Chat(state) = &mut app.screen {
                state.env_prompt_input.clear();
                state.env_prompt_idx += 1;
                if !state.has_env_prompt() {
                    state.env_prompt_vars.clear();
                    state.env_prompt_idx = 0;
                    // Refresh banner env status after all vars prompted.
                    start_env_check(app);
                }
            }
        }
        KeyCode::Esc if has_env_prompt => {
            let remaining = state.env_prompt_vars.len() - state.env_prompt_idx;
            state.env_prompt_vars.clear();
            state.env_prompt_idx = 0;
            state.env_prompt_input.clear();
            set_toast(
                app,
                &format!("Skipped {remaining} env var(s). /config set KEY=VALUE later."),
            );
        }
        KeyCode::Char(c) if has_env_prompt => {
            state.env_prompt_input.push(c);
        }
        KeyCode::Backspace if has_env_prompt => {
            state.env_prompt_input.pop();
        }
        // --- Dynamic picker navigation ---
        KeyCode::Up if has_dynamic && !state.dynamic_loading => {
            if state.dynamic_selected > 0 {
                state.dynamic_selected -= 1;
            } else if !state.dynamic_matches.is_empty() {
                // At top — confirm selection (same as Enter).
                let item_idx = state.dynamic_matches[state.dynamic_selected];
                let item = &state.dynamic_items[item_idx];
                if item.disabled {
                    return Ok(());
                }
                let cmd = state.dynamic_command.clone().unwrap();
                let full = format!("/{} {}", cmd, item.value);
                state.dynamic_items.clear();
                state.dynamic_matches.clear();
                state.dynamic_selected = 0;
                state.dynamic_command = None;
                state.dynamic_loading = false;
                state.active_command = None;
                state.sub_matches.clear();
                state.sub_selected = 0;
                state.slash_matches.clear();
                state.slash_selected = 0;
                state.input.clear();
                dispatch_slash(app, &full).await?;
            }
        }
        KeyCode::Down if has_dynamic && !state.dynamic_loading => {
            if state.dynamic_selected + 1 < state.dynamic_matches.len() {
                state.dynamic_selected += 1;
            }
        }
        KeyCode::Enter if has_dynamic && !state.dynamic_loading && !state.dynamic_matches.is_empty() => {
            let item_idx = state.dynamic_matches[state.dynamic_selected];
            let item = &state.dynamic_items[item_idx];
            if item.disabled {
                return Ok(());
            }
            let cmd = state.dynamic_command.clone().unwrap();
            let full = format!("/{} {}", cmd, item.value);
            // Clear dynamic picker state.
            state.dynamic_items.clear();
            state.dynamic_matches.clear();
            state.dynamic_selected = 0;
            state.dynamic_command = None;
            state.dynamic_loading = false;
            state.active_command = None;
            state.sub_matches.clear();
            state.sub_selected = 0;
            state.slash_matches.clear();
            state.slash_selected = 0;
            state.input.clear();
            dispatch_slash(app, &full).await?;
        }
        KeyCode::Char(c) if has_dynamic && !state.dynamic_loading => {
            state.input.push(c);
            update_dynamic_filter(state);
        }
        KeyCode::Backspace if has_dynamic && !state.dynamic_loading => {
            state.input.pop();
            update_dynamic_filter(state);
        }
        KeyCode::Esc if has_dynamic => {
            state.dynamic_items.clear();
            state.dynamic_matches.clear();
            state.dynamic_selected = 0;
            state.dynamic_command = None;
            state.dynamic_loading = false;
            state.active_command = None;
            state.sub_matches.clear();
            state.sub_selected = 0;
            state.slash_matches.clear();
            state.slash_selected = 0;
            state.input.clear();
            app.pending_components = None;
        }
        // --- Sub-menu navigation ---
        KeyCode::Up if has_sub_menu => {
            if state.sub_selected > 0 {
                state.sub_selected -= 1;
            } else {
                // At top — confirm selection (same as Enter/Tab).
                let cmd_idx = state.active_command.unwrap();
                let sub_idx = state.sub_matches[state.sub_selected];
                let sub = &SLASH_COMMANDS[cmd_idx].subs[sub_idx];
                let cmd_name = SLASH_COMMANDS[cmd_idx].name;
                if sub.needs_arg && cmd_name == "tools" {
                    let sub_name = sub.name.to_string();
                    state.input.clear();
                    state.active_command = None;
                    state.sub_matches.clear();
                    state.sub_selected = 0;
                    state.slash_matches.clear();
                    state.slash_selected = 0;
                    if sub_name == "add" {
                        state.dynamic_command = Some("tools add".to_string());
                        state.dynamic_loading = true;
                        start_component_fetch(app);
                    } else if sub_name == "remove" {
                        state.dynamic_command = Some("tools remove".to_string());
                        state.dynamic_items = build_remove_items(&agent_tools);
                        state.dynamic_matches =
                            (0..state.dynamic_items.len()).collect();
                        state.dynamic_selected = 0;
                    }
                } else if sub.needs_arg {
                    state.input = format!("/{cmd_name} {} ", sub.name);
                    state.active_command = None;
                    state.sub_matches.clear();
                    state.sub_selected = 0;
                    state.slash_matches.clear();
                    state.slash_selected = 0;
                } else {
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
            if sub.needs_arg && cmd_name == "tools" {
                // Dynamic picker for /tools add and /tools remove.
                let sub_name = sub.name.to_string();
                state.input.clear();
                state.active_command = None;
                state.sub_matches.clear();
                state.sub_selected = 0;
                state.slash_matches.clear();
                state.slash_selected = 0;
                if sub_name == "add" {
                    state.dynamic_command = Some("tools add".to_string());
                    state.dynamic_loading = true;
                    start_component_fetch(app);
                } else if sub_name == "remove" {
                    state.dynamic_command = Some("tools remove".to_string());
                    state.dynamic_items = build_remove_items(&agent_tools);
                    state.dynamic_matches =
                        (0..state.dynamic_items.len()).collect();
                    state.dynamic_selected = 0;
                }
            } else if sub.needs_arg {
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
            } else if state.slash_selected < state.slash_matches.len() {
                // At top — confirm selection (same as Enter/Tab).
                let cmd_idx = state.slash_matches[state.slash_selected];
                let cmd = &SLASH_COMMANDS[cmd_idx];
                if !cmd.subs.is_empty() {
                    state.input = format!("/{} ", cmd.name);
                    state.active_command = Some(cmd_idx);
                    state.sub_matches = (0..cmd.subs.len()).collect();
                    state.sub_selected = 0;
                    state.slash_matches.clear();
                    state.slash_selected = 0;
                } else {
                    state.input = format!("/{}", cmd.name);
                    state.slash_matches.clear();
                    state.slash_selected = 0;
                    let input = state.input.clone();
                    state.input.clear();
                    dispatch_slash(app, &input).await?;
                }
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
            state.input.clear();
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
            if !state.input.is_empty() {
                // Clear input first.
                state.input.clear();
                state.slash_matches.clear();
                state.slash_selected = 0;
            } else {
                // Empty input — return to agent picker.
                app.agent = None;
                app.pending_response = None;
                app.pending_banner = None;
                app.pending_components = None;
                app.pending_auto_start = None;
                // Restore saved picker state instantly (no loading screen).
                if let Some((agents, orphans)) = app.saved_picker.take() {
                    app.screen = Screen::Picker(PickerState {
                        agents,
                        selected: 0,
                        loading: false,
                        error: None,
                        running_agents: orphans,
                        starting_agent: None,
                        spinner_tick: 0,
                    });
                    // Fire async process scan to refresh running status.
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    app.pending_process_scan = Some(rx);
                    tokio::spawn(async move {
                        let result = tokio::task::spawn_blocking(
                            crate::tui::ops::scan_running_agents,
                        )
                        .await
                        .unwrap_or_default();
                        let _ = tx.send(result);
                    });
                } else {
                    app.screen = Screen::Picker(PickerState::loading(0));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn render_banner(
    f: &mut Frame,
    name: &str,
    chat: &ChatState,
    app: &App,
    area: Rect,
) {
    let agent = app.agent.as_ref();
    let process_starting = app.pending_auto_start.is_some();
    let banner_text = &chat.banner_text;
    let banner_loading = chat.banner_loading;
    let model = agent.and_then(|b| b.model.as_deref()).unwrap_or("not set");
    let tools = agent.map(|b| &b.tools).cloned().unwrap_or_default();
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
        .unwrap_or_else(|| "USER");
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
    let label_style = Style::default().fg(Color::Rgb(120, 120, 140));
    let value_style = Style::default().fg(Color::Rgb(180, 180, 200));
    let tool_green = Style::default().fg(Color::Rgb(80, 220, 120));
    let tool_orange = Style::default().fg(Color::Rgb(255, 165, 0));
    let tool_plain = Style::default().fg(Color::Rgb(140, 140, 160));
    let sep_style = Style::default().fg(Color::Rgb(80, 80, 100));
    let env_status = &chat.tool_env_status;
    let env_loaded = !env_status.is_empty() || app.pending_env_check.is_none();

    let mut right_lines: Vec<Line> = Vec::new();
    right_lines.push(Line::from(vec![
        Span::styled("MODEL   ", label_style),
        Span::styled(model, value_style),
    ]));
    // Tools: inline across the page, color-coded by env var status.
    if tools.is_empty() {
        right_lines.push(Line::from(vec![
            Span::styled("TOOLS   ", label_style),
            Span::styled("none", Style::default().fg(Color::DarkGray)),
        ]));
    } else {
        // Build styled spans for each tool name.
        let mut tool_spans: Vec<(String, Style)> = Vec::new();
        for full_name in &tools {
            let short = full_name.split(':').next_back().unwrap_or(full_name).to_string();
            let env_vars = required_env_vars(full_name);
            let style = if env_vars.is_empty() {
                tool_plain
            } else if !env_loaded {
                tool_plain
            } else {
                let all_set = env_vars.iter().all(|v| {
                    env_status.get(*v).copied().unwrap_or(false)
                });
                if all_set { tool_green } else { tool_orange }
            };
            tool_spans.push((short, style));
        }
        // Wrap tool names across lines to fit available width.
        let label_w = 8; // "TOOLS   ".len()
        let avail = right_area.width.saturating_sub(2) as usize;
        let tool_max = avail.saturating_sub(label_w);
        let sep = " · ";
        let mut first_line = true;
        let mut line_spans: Vec<Span> = Vec::new();
        let mut line_len: usize = 0;
        for (i, (name, style)) in tool_spans.iter().enumerate() {
            let needed = if i == 0 || line_len == 0 { name.len() } else { sep.len() + name.len() };
            if line_len > 0 && line_len + needed > tool_max {
                // Flush current line.
                let prefix = if first_line { "TOOLS   " } else { "        " };
                let mut spans = vec![Span::styled(prefix, label_style)];
                spans.append(&mut line_spans);
                right_lines.push(Line::from(spans));
                line_spans = Vec::new();
                line_len = 0;
                first_line = false;
            }
            if line_len > 0 {
                line_spans.push(Span::styled(sep, sep_style));
                line_len += sep.len();
            }
            line_spans.push(Span::styled(name.clone(), *style));
            line_len += name.len();
        }
        if !line_spans.is_empty() {
            let prefix = if first_line { "TOOLS   " } else { "        " };
            let mut spans = vec![Span::styled(prefix, label_style)];
            spans.append(&mut line_spans);
            right_lines.push(Line::from(spans));
        }
    }
    if dirs_count > 0 {
        let dirs_label = match dirs_count {
            1 => "1 folder".to_string(),
            n => format!("{n} folders"),
        };
        right_lines.push(Line::from(vec![
            Span::styled("DIRS    ", label_style),
            Span::styled(dirs_label, value_style),
        ]));
    }
    // Show running process info.
    if let Some(ra) = &chat.running_process {
        right_lines.push(Line::from(vec![
            Span::styled("PROCESS ", label_style),
            Span::styled(
                format!(":{} (pid {})", ra.port, ra.pid),
                Style::default().fg(Color::Rgb(80, 220, 120)),
            ),
        ]));
    } else if process_starting {
        let frame = crate::tui::app::SPINNER_FRAMES[chat.spinner_tick % crate::tui::app::SPINNER_FRAMES.len()];
        right_lines.push(Line::from(vec![
            Span::styled("PROCESS ", label_style),
            Span::styled(
                format!("{frame} starting..."),
                Style::default().fg(Color::Rgb(255, 200, 60)),
            ),
        ]));
    }
    right_lines.push(Line::from(""));
    right_lines.push(Line::from(vec![
        Span::styled("Type ", Style::default().fg(Color::Rgb(90, 90, 110))),
        Span::styled("/", Style::default().fg(Color::Cyan)),
        Span::styled(" for commands · ", Style::default().fg(Color::Rgb(90, 90, 110))),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::styled(" to go back", Style::default().fg(Color::Rgb(90, 90, 110))),
    ]));
    // Banner content (quote or tool data).
    if !banner_text.is_empty() {
        right_lines.push(Line::from(""));
        let display = match banner_loading {
            true => format!("{banner_text} ..."),
            false => banner_text.to_string(),
        };
        let display = match display.len() > 120 {
            true => format!("{}...", &display[..117]),
            false => display,
        };
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
        if let Some(styled) = &msg.styled_lines {
            lines.extend(styled.iter().cloned());
        } else {
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
    // Blinking cursor: visible on even ticks, hidden on odd.
    let cursor_visible = state.spinner_tick % 2 == 0;
    let cursor_ch = if cursor_visible { "_" } else { " " };
    let w = area.width as usize;
    if w == 0 {
        return;
    }
    if state.has_env_prompt() {
        let var_name = &state.env_prompt_vars[state.env_prompt_idx];
        let remaining = state.env_prompt_vars.len() - state.env_prompt_idx;
        let ghost = Style::default().fg(Color::Rgb(60, 60, 70));
        let hint_text = if remaining > 1 {
            format!("  ({remaining} remaining, Esc to skip)")
        } else {
            "  (Esc to skip)".to_string()
        };
        let placeholder = if state.env_prompt_input.is_empty() {
            "paste your API key here"
        } else {
            ""
        };
        let mut spans: Vec<Span<'static>> = vec![
            Span::styled(
                format!("{var_name}="),
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::styled(
                state.env_prompt_input.clone(),
                Style::default().fg(Color::White),
            ),
            Span::styled(cursor_ch.to_string(), Style::default().fg(Color::White)),
        ];
        if !placeholder.is_empty() {
            spans.push(Span::styled(placeholder.to_string(), ghost));
        }
        spans.push(Span::styled(hint_text, Style::default().fg(Color::White)));
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    } else if state.input.is_empty() {
        // Ghost placeholder text with blinking cursor.
        let ghost = Style::default().fg(Color::Rgb(60, 60, 70));
        let line = Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::styled(cursor_ch.to_string(), Style::default().fg(Color::White)),
            Span::styled(" start typing here or ", ghost),
            Span::styled(" / ", ghost.bold()),
            Span::styled("for commands", ghost),
        ]);
        f.render_widget(Paragraph::new(line), area);
    } else {
        let full = format!("> {}{cursor_ch}", state.input);
        let lines = wrap_input_chars(&full, w);
        let mut out: Vec<Line<'static>> = Vec::new();
        for chunk in lines {
            if out.is_empty() {
                // First line: color "> " cyan.
                out.push(Line::from(vec![
                    Span::styled("> ".to_string(), Style::default().fg(Color::Cyan)),
                    Span::raw(chunk[2..].to_string()),
                ]));
            } else {
                out.push(Line::from(Span::raw(chunk)));
            }
        }
        f.render_widget(Paragraph::new(out), area);
    }
}

/// Break a string into chunks of at most `w` characters.
fn wrap_input_chars(s: &str, w: usize) -> Vec<String> {
    if w == 0 {
        return vec![s.to_string()];
    }
    let chars: Vec<char> = s.chars().collect();
    chars.chunks(w).map(|c| c.iter().collect()).collect()
}

fn render_slash_menu(f: &mut Frame, state: &ChatState, area: Rect) {
    let max_rows = area.height as usize;

    // Dynamic picker mode: show browsable items.
    if state.dynamic_command.is_some() {
        if state.dynamic_loading {
            let line = Line::from(vec![
                Span::raw("  "),
                Span::styled("  Loading...", Style::default().fg(Color::DarkGray).italic()),
            ]);
            f.render_widget(Paragraph::new(vec![line]), area);
            return;
        }
        if state.dynamic_matches.is_empty() {
            let line = Line::from(vec![
                Span::raw("  "),
                Span::styled("  (no matches)", Style::default().fg(Color::DarkGray)),
            ]);
            f.render_widget(Paragraph::new(vec![line]), area);
            return;
        }
        let skip = state
            .dynamic_selected
            .saturating_sub(max_rows.saturating_sub(1));
        let mut lines: Vec<Line> = Vec::new();
        for (i, &item_idx) in state
            .dynamic_matches
            .iter()
            .enumerate()
            .skip(skip)
            .take(max_rows)
        {
            let item = &state.dynamic_items[item_idx];
            let is_selected = i == state.dynamic_selected;
            let pointer = match is_selected {
                true => "▸ ",
                false => "  ",
            };
            let (name_style, desc_style) = if item.disabled {
                (
                    Style::default().fg(Color::DarkGray),
                    Style::default().fg(Color::DarkGray),
                )
            } else {
                (
                    match is_selected {
                        true => Style::default().fg(Color::Cyan).bold(),
                        false => Style::default().fg(Color::Cyan),
                    },
                    match is_selected {
                        true => Style::default().fg(Color::White),
                        false => Style::default().fg(Color::DarkGray),
                    },
                )
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::raw(pointer),
                Span::styled(format!("{:<20}", item.label), name_style),
                Span::styled(&item.description, desc_style),
            ]));
        }
        f.render_widget(Paragraph::new(lines), area);
        return;
    }

    // Sub-menu mode: show sub-options for the active command.
    if let Some(cmd_idx) = state.active_command {
        let cmd = &SLASH_COMMANDS[cmd_idx];
        let skip = state.sub_selected.saturating_sub(max_rows.saturating_sub(1));
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

fn render_info_overlay(f: &mut Frame, state: &ChatState, area: Rect) {
    let lines = match &state.info_overlay {
        Some(lines) => lines,
        None => return,
    };
    // Size overlay to fit content, capped at the available message area.
    // +2 for top/bottom borders.
    let content_h = (lines.len() as u16) + 2;
    let h = content_h.min(area.height);
    let overlay = Rect::new(
        area.x + 1,
        area.y + area.height.saturating_sub(h),
        area.width.saturating_sub(2),
        h,
    );
    f.render_widget(Clear, overlay);
    let block = Block::default()
        .title(" Info ")
        .title_alignment(Alignment::Left)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);
    let total_lines = lines.len() as u16;
    let scroll = state.info_overlay_scroll;
    let content = Paragraph::new(lines.clone()).scroll((scroll, 0));
    f.render_widget(content, inner);
    // Show dismiss hint on the bottom border.
    let hint = if total_lines > inner.height {
        " ↑↓ scroll · any key to close "
    } else {
        " any key to close "
    };
    let hint_w = hint.len() as u16;
    if hint_w + 2 < overlay.width {
        let hint_area = Rect::new(
            overlay.x + overlay.width.saturating_sub(hint_w + 1),
            overlay.y + overlay.height.saturating_sub(1),
            hint_w,
            1,
        );
        f.render_widget(
            Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
            hint_area,
        );
    }
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
        let result = match tokio::task::spawn(async move {
            ops::call_converse(prompt, &agent).await
        })
        .await
        {
            Ok(r) => r.ok().flatten(),
            Err(_) => None,
        };
        let _ = tx.send(result);
    });
}

/// Kick off an async env-var check so the banner can show per-tool status.
pub fn start_env_check(app: &mut App) {
    let Some(agent) = &app.agent else { return };
    let env_name = agent.env_name.clone();
    let tools = agent.tools.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.pending_env_check = Some(rx);
    tokio::spawn(async move {
        let var_values = ops::inspect_environment(&env_name)
            .await
            .ok()
            .flatten()
            .map(|d| d.var_values)
            .unwrap_or_default();
        let mut status = std::collections::HashMap::new();
        for tool in &tools {
            for v in required_env_vars(tool) {
                status.insert(v.to_string(), var_values.contains_key(*v));
            }
        }
        let _ = tx.send(status);
    });
}

fn send_message(app: &mut App, input: &str) {
    let Screen::Chat(state) = &mut app.screen else {
        return;
    };
    state.messages.push(ChatMessage {
        role: MessageRole::User,
        content: input.to_string(),
        styled_lines: None,
    });
    state.waiting = true;
    let agent = app.agent.clone();
    let running = state.running_process.clone();
    let message = input.to_string();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.pending_response = Some(rx);
    tokio::spawn(async move {
        let result = match agent {
            Some(ref a) => {
                // Route through running process if available.
                if let Some(ra) = &running {
                    let ns = a.namespace.clone();
                    let env = a.env_name.clone();
                    let port = ra.port;
                    let msg = message.clone();
                    ops::call_converse_via_process(&msg, &ns, &env, port).await
                } else {
                    let a = a.clone();
                    let msg = message.clone();
                    match tokio::task::spawn(async move {
                        ops::call_converse(&msg, &a).await
                    })
                    .await
                    {
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
        "me" | "whoami" => cmd_me(app, args),
        "dir" | "dirs" => cmd_dir(app, args),
        "status" | "info" => cmd_status(app).await,
        "banner" => cmd_banner(app, args),
        "push" => cmd_push(app).await,
        "pull" | "sync" => cmd_pull(app).await,
        "config" | "vars" => cmd_config(app, args).await,
        "quit" | "exit" | "q" => {
            app.should_quit = true;
            Ok(())
        }
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
                state.sub_selected = state.sub_selected.min(
                    state.sub_matches.len().saturating_sub(1),
                );
            }
        } else {
            // Input no longer matches the command prefix — exit sub-menu.
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
        state.slash_selected = state.slash_selected.min(
            state.slash_matches.len().saturating_sub(1),
        );
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
            styled_lines: None,
        });
    }
}

/// Show an ephemeral toast on the separator line (auto-dismisses after ~3s).
fn set_toast(app: &mut App, msg: &str) {
    set_toast_color(app, msg, Color::Yellow);
}

fn set_toast_color(app: &mut App, msg: &str, color: Color) {
    if let Screen::Chat(state) = &mut app.screen {
        state.toast = Some(msg.to_string());
        state.toast_color = color;
        state.toast_until = Some(Instant::now() + Duration::from_secs(3));
    }
}

fn cmd_help(app: &mut App) -> eyre::Result<()> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled(
        "Commands:",
        Style::default().fg(Color::Yellow).bold(),
    )));
    for cmd in SLASH_COMMANDS {
        lines.push(Line::from(vec![
            Span::styled(format!("  /{:<12}", cmd.name), Style::default().fg(Color::Cyan)),
            Span::styled(cmd.description.to_string(), Style::default().fg(Color::White)),
        ]));
        for sub in cmd.subs {
            lines.push(Line::from(vec![
                Span::styled(format!("    {:<10}", sub.name), Style::default().fg(Color::DarkGray)),
                Span::styled(sub.description.to_string(), Style::default().fg(Color::DarkGray)),
            ]));
        }
    }
    app.show_info_overlay(lines);
    Ok(())
}

async fn cmd_tools(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    let tools = app
        .agent
        .as_ref()
        .map(|b| &b.tools)
        .cloned()
        .unwrap_or_default();
    if args.is_empty() || args[0] == "list" {
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::from(Span::styled(
            "Enabled tools:",
            Style::default().fg(Color::Yellow).bold(),
        )));
        if tools.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (none)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            let env_name = app.agent.as_ref().map(|a| a.env_name.clone());
            let var_values = if let Some(ref name) = env_name {
                ops::inspect_environment(name)
                    .await
                    .ok()
                    .flatten()
                    .map(|d| d.var_values)
                    .unwrap_or_default()
            } else {
                std::collections::HashMap::new()
            };
            let mut warnings: Vec<Line<'static>> = Vec::new();
            for tool in &tools {
                lines.push(Line::from(Span::styled(
                    format!("  {tool}"),
                    Style::default().fg(Color::White),
                )));
                let needed = required_env_vars(tool);
                let missing: Vec<&&str> = needed
                    .iter()
                    .filter(|v| !var_values.contains_key(**v))
                    .collect();
                if !missing.is_empty() {
                    let vars = missing.iter().map(|v| **v).collect::<Vec<_>>().join(", ");
                    warnings.push(Line::from(Span::styled(
                        format!("  {tool}: {vars}"),
                        Style::default().fg(Color::Rgb(255, 165, 0)),
                    )));
                }
            }
            if !warnings.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "\u{26A0} Missing config:",
                    Style::default().fg(Color::Rgb(255, 165, 0)),
                )));
                lines.extend(warnings);
                lines.push(Line::from(Span::styled(
                    "  Use /config set KEY=VALUE to configure",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "/tools add <ns:component>  Add a tool",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "/tools remove <component>  Remove a tool",
            Style::default().fg(Color::DarkGray),
        )));
        app.show_info_overlay(lines);
        return Ok(());
    }
    let Some(agent) = &app.agent else {
        push_system(app, "No active agent.");
        return Ok(());
    };
    let env_name = agent.env_name.clone();
    if args[0] == "add" && args.len() > 1 {
        // Auto-prepend user namespace if missing (e.g. "trello" → "seadog:trello").
        let component = if args[1].contains(':') {
            args[1].to_string()
        } else {
            let ns = crate::auth::Auth::read_user_or_fallback_namespace();
            format!("{ns}:{}", args[1])
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
                let env_vars = required_env_vars(&component);
                if env_vars.is_empty() {
                    set_toast(app, &format!("+ {component}"));
                    start_env_check(app);
                } else {
                    // Check which vars are already set.
                    let data = ops::inspect_environment(&env_name).await.ok().flatten();
                    let var_values = data.map(|d| d.var_values).unwrap_or_default();
                    let missing: Vec<String> = env_vars
                        .iter()
                        .filter(|v| !var_values.contains_key(**v))
                        .map(|v| v.to_string())
                        .collect();
                    if missing.is_empty() {
                        set_toast(app, &format!("+ {component}"));
                    } else {
                        let short = component.split(':').next_back().unwrap_or(&component);
                        set_toast(
                            app,
                            &format!("+ {short} (configure env vars below)"),
                        );
                        // Start the inline env-var prompt flow.
                        if let Screen::Chat(state) = &mut app.screen {
                            state.env_prompt_vars = missing;
                            state.env_prompt_idx = 0;
                            state.env_prompt_input.clear();
                        }
                    }
                }
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
                // Clean up env vars associated with the removed tool.
                let env_vars = required_env_vars(component);
                let mut removed_vars: Vec<&str> = Vec::new();
                for v in env_vars {
                    if ops::set_var(&env_name, v, "").is_ok() {
                        removed_vars.push(v);
                    }
                }
                if removed_vars.is_empty() {
                    set_toast(app, &format!("- {component}"));
                } else {
                    let short = component.split(':').next_back().unwrap_or(component);
                    set_toast(
                        app,
                        &format!("- {short} (cleared {})", removed_vars.join(", ")),
                    );
                }
                start_env_check(app);
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
    set_toast(app, "Conversation cleared.");
    Ok(())
}

fn cmd_model(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    if args.is_empty() {
        // Open the model picker.
        let current_model = app.agent.as_ref().and_then(|b| b.model.clone());
        let items = build_model_items(current_model.as_deref());
        // Pre-select the current model.
        let selected = current_model
            .as_deref()
            .and_then(|m| items.iter().position(|item| item.value == m))
            .unwrap_or(0);
        if let Screen::Chat(state) = &mut app.screen {
            state.dynamic_command = Some("model".to_string());
            state.dynamic_items = items;
            state.dynamic_matches = (0..state.dynamic_items.len()).collect();
            state.dynamic_selected = selected;
            state.dynamic_loading = false;
            state.input.clear();
        }
        return Ok(());
    }
    let new_model = args[0].to_string();
    if let Some(agent) = &app.agent {
        let _ = ops::set_var(&agent.env_name, "ASTERBOT_MODEL", &new_model);
    }
    if let Some(agent) = &mut app.agent {
        agent.model = Some(new_model.clone());
    }
    set_toast(app, &format!("Model set to {new_model}"));
    Ok(())
}

fn cmd_name(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    if args.is_empty() {
        let name = app
            .agent
            .as_ref()
            .map(|b| b.bot_name.as_str())
            .unwrap_or("Asterbot");
        app.show_info_overlay(vec![
            Line::from(vec![
                Span::styled("Agent name: ", Style::default().fg(Color::Yellow)),
                Span::styled(name.to_string(), Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Change with: /name <new name>",
                Style::default().fg(Color::DarkGray),
            )),
        ]);
        return Ok(());
    }
    let new_name = args.join(" ");
    if let Some(agent) = &app.agent {
        let _ = ops::set_var(&agent.env_name, "ASTERBOT_BOT_NAME", &new_name);
    }
    if let Some(agent) = &mut app.agent {
        agent.bot_name = new_name.clone();
    }
    set_toast(app, &format!("Agent renamed to {new_name}"));
    Ok(())
}

fn cmd_me(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    if args.is_empty() {
        let name = app
            .agent
            .as_ref()
            .map(|b| b.user_name.as_str())
            .unwrap_or("not set");
        app.show_info_overlay(vec![
            Line::from(vec![
                Span::styled("Display name: ", Style::default().fg(Color::Yellow)),
                Span::styled(name.to_string(), Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Change with: /me <name>",
                Style::default().fg(Color::DarkGray),
            )),
        ]);
        return Ok(());
    }
    let new_name = args.join(" ");
    if let Some(agent) = &app.agent {
        let _ = ops::set_var(&agent.env_name, "ASTERBOT_USER_NAME", &new_name);
    }
    if let Some(agent) = &mut app.agent {
        agent.user_name = new_name.clone();
    }
    set_toast(app, &format!("Display name set to {new_name}"));
    Ok(())
}

fn cmd_banner(app: &mut App, args: &[&str]) -> eyre::Result<()> {
    if args.is_empty() {
        let mode = app
            .agent
            .as_ref()
            .map(|a| a.banner_mode.as_str())
            .unwrap_or("auto");
        app.show_info_overlay(vec![
            Line::from(vec![
                Span::styled("Banner mode: ", Style::default().fg(Color::Yellow)),
                Span::styled(mode.to_string(), Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "/banner auto   Agent picks content from tools",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "/banner quote  Random quotes only",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "/banner off    No banner content",
                Style::default().fg(Color::DarkGray),
            )),
        ]);
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
            set_toast(app, "Banner: auto (fetching from tools)");
            start_banner_fetch(app);
        }
        "quote" => {
            if let Screen::Chat(state) = &mut app.screen {
                state.banner_text = crate::tui::app::random_quote().to_string();
                state.banner_loading = false;
            }
            set_toast(app, "Banner: random quotes");
        }
        "off" => {
            if let Screen::Chat(state) = &mut app.screen {
                state.banner_text.clear();
                state.banner_loading = false;
            }
            set_toast(app, "Banner: off");
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
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::from(Span::styled(
            "Allowed directories:",
            Style::default().fg(Color::Yellow).bold(),
        )));
        if dirs.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (none)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for dir in &dirs {
                lines.push(Line::from(Span::styled(
                    format!("  {dir}"),
                    Style::default().fg(Color::White),
                )));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "/dir add <path>     Grant access",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "/dir remove <path>  Revoke access",
            Style::default().fg(Color::DarkGray),
        )));
        app.show_info_overlay(lines);
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
        set_toast(app, &format!("+ {resolved}"));
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
        set_toast(app, &format!("- {resolved}"));
        return Ok(());
    }
    push_system(app, "Usage: /dir [list|add|remove] <path>");
    Ok(())
}

async fn cmd_status(app: &mut App) -> eyre::Result<()> {
    let Some(agent) = &app.agent else {
        push_system(app, "No active agent.");
        return Ok(());
    };

    let heading = Style::default().fg(Color::Yellow).bold();
    let label = Style::default().fg(Color::Yellow);
    let value = Style::default().fg(Color::White);
    let tool_name_style = Style::default().fg(Color::Rgb(200, 180, 80));
    let ok = Style::default().fg(Color::Green);
    let warn = Style::default().fg(Color::Rgb(255, 165, 0)); // orange

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled("Agent Status:", heading)));
    lines.push(Line::from(vec![
        Span::styled("  Name:        ", label),
        Span::styled(agent.bot_name.clone(), value),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Environment: ", label),
        Span::styled(agent.env_name.clone(), value),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Model:       ", label),
        Span::styled(
            agent.model.clone().unwrap_or_else(|| "(not set)".to_string()),
            value,
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Provider:    ", label),
        Span::styled(agent.provider.clone(), value),
    ]));

    if !agent.tools.is_empty() {
        // Fetch env var values for status display.
        let data = ops::inspect_environment(&agent.env_name).await.ok().flatten();
        let var_values = data.map(|d| d.var_values).unwrap_or_default();
        lines.push(Line::from(Span::styled("  Tools:", label)));
        for tool in &agent.tools {
            let short = tool.split(':').next_back().unwrap_or(tool).to_string();
            let env_vars = required_env_vars(tool);
            if env_vars.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("    {short}"),
                    tool_name_style,
                )));
            } else {
                let padded = format!("{:<16}", short);
                let mut spans: Vec<Span<'static>> =
                    vec![Span::styled(format!("    {padded} "), tool_name_style)];
                for (i, v) in env_vars.iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::styled(", ", tool_name_style));
                    }
                    if var_values.contains_key(*v) {
                        spans.push(Span::styled(
                            format!("{v} \u{2713}"),
                            ok,
                        ));
                    } else {
                        spans.push(Span::styled(
                            format!("{v} \u{2717}"),
                            warn,
                        ));
                    }
                }
                lines.push(Line::from(spans));
            }
        }
    }
    if !agent.allowed_dirs.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Directories: ", label),
            Span::styled(
                format!("{} folder(s)", agent.allowed_dirs.len()),
                value,
            ),
        ]));
    }
    app.show_info_overlay(lines);
    Ok(())
}

async fn cmd_push(app: &mut App) -> eyre::Result<()> {
    let Some(agent) = &app.agent else {
        push_system(app, "No active agent.");
        return Ok(());
    };
    let env_name = agent.env_name.clone();
    set_toast(app, &format!("Pushing {env_name}..."));
    match ops::push_env(&env_name).await {
        Ok(_) => set_toast(app, &format!("Pushed {env_name} to cloud.")),
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
    set_toast(app, &format!("Pulling {env_name}..."));
    match ops::pull_env(&env_name).await {
        Ok(_) => set_toast(app, &format!("Pulled {env_name} from cloud.")),
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
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::from(Span::styled(
            format!("Environment variables ({env_name}):"),
            Style::default().fg(Color::Yellow).bold(),
        )));
        if vars.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (none)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for v in &vars {
                lines.push(Line::from(Span::styled(
                    format!("  {v}"),
                    Style::default().fg(Color::White),
                )));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "/config set KEY=VALUE  Set a variable",
            Style::default().fg(Color::DarkGray),
        )));
        app.show_info_overlay(lines);
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
            Ok(_) => {
                set_toast_color(app, &format!("{key} set."), Color::Green);
                start_env_check(app);
            }
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

fn start_component_fetch(app: &mut App) {
    let installed: Vec<String> = app
        .agent
        .as_ref()
        .map(|a| {
            a.tools
                .iter()
                .map(|t| t.split('@').next().unwrap_or(t).to_string())
                .collect()
        })
        .unwrap_or_default();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.pending_components = Some(rx);
    tokio::spawn(async move {
        let result = ops::list_remote_components().await.map(|components| {
            components
                .into_iter()
                .filter(|(ns, name, _)| {
                    let ref_str = format!("{ns}:{name}");
                    !installed.contains(&ref_str)
                })
                .map(|(ns, name, _ver)| DynamicItem {
                    value: format!("{ns}:{name}"),
                    label: name,
                    description: ns,
                    disabled: false,
                })
                .collect()
        });
        let _ = tx.send(result);
    });
}

/// Build the DynamicItem list for /tools remove: core components (disabled) + user tools.
fn build_remove_items(tools: &[String]) -> Vec<DynamicItem> {
    let mut items: Vec<DynamicItem> = CORE_COMPONENTS
        .iter()
        .map(|c| {
            let label = c.split(':').next_back().unwrap_or(c).to_string();
            let ns = c.split(':').next().unwrap_or("").to_string();
            DynamicItem {
                value: c.to_string(),
                label,
                description: format!("{ns} (core)"),
                disabled: true,
            }
        })
        .collect();
    for t in tools {
        let label = t.split(':').next_back().unwrap_or(t).to_string();
        let ns = t.split(':').next().unwrap_or("").to_string();
        items.push(DynamicItem {
            value: t.clone(),
            label,
            description: ns,
            disabled: false,
        });
    }
    items
}

/// Build the DynamicItem list for /model: all models from PROVIDERS grouped by provider.
fn build_model_items(current_model: Option<&str>) -> Vec<DynamicItem> {
    let mut items = vec![DynamicItem {
        value: String::new(),
        label: "asterai managed LLM (coming soon)".to_string(),
        description: "asterai".to_string(),
        disabled: true,
    }];
    for &(provider_name, _key, models) in PROVIDERS {
        for &(model_id, model_label) in models {
            let is_current = current_model.map_or(false, |m| m == model_id);
            let label = if is_current {
                format!("{model_label} *")
            } else {
                model_label.to_string()
            };
            items.push(DynamicItem {
                value: model_id.to_string(),
                label,
                description: provider_name.to_string(),
                disabled: false,
            });
        }
    }
    items
}

fn update_dynamic_filter(state: &mut ChatState) {
    let filter = state.input.to_lowercase();
    if filter.is_empty() {
        state.dynamic_matches = (0..state.dynamic_items.len()).collect();
    } else {
        state.dynamic_matches = state
            .dynamic_items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.label.to_lowercase().contains(&filter)
                    || item.value.to_lowercase().contains(&filter)
            })
            .map(|(i, _)| i)
            .collect();
    }
    state.dynamic_selected = state
        .dynamic_selected
        .min(state.dynamic_matches.len().saturating_sub(1));
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

