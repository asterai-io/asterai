use crate::tui::Tty;
use crate::tui::app::{App, AuthState, PickerState, Screen};
use crate::tui::ops;
use crate::tui::views::picker;
use crossterm::event::{Event, KeyCode};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn render(f: &mut Frame, state: &AuthState) {
    let has_error = matches!(state, AuthState::NeedLogin { error: Some(_), .. });
    let height = match has_error {
        true => 14,
        false => 12,
    };
    let area = centered_rect(60, height, f.area());
    let block = Block::default()
        .title(" asterai agents ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);
    match state {
        AuthState::Checking => {
            let text = Paragraph::new("Checking login...")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            f.render_widget(text, inner);
        }
        AuthState::LoggingIn => {
            let text = Paragraph::new("Logging in...")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            f.render_widget(text, inner);
        }
        AuthState::NeedLogin { input, error } => {
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "You need an asterai account to continue.",
                    Style::default().bold(),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Sign up:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled("https://asterai.io/login", Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("API keys:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        "https://asterai.io/settings/api-keys",
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
                Line::from(""),
            ];
            if let Some(err) = error {
                lines.push(Line::from(Span::styled(
                    err.as_str(),
                    Style::default().fg(Color::Red).bold(),
                )));
                lines.push(Line::from(Span::styled(
                    "Check your key at asterai.io/settings/api-keys",
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(""));
            }
            let masked: String = "*".repeat(input.len());
            lines.push(Line::from(vec![
                Span::styled("API key: ", Style::default().bold()),
                Span::styled(masked, Style::default().fg(Color::Yellow)),
                Span::styled("_", Style::default().fg(Color::DarkGray)),
            ]));
            let text = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(text, inner);
        }
    }
}

pub async fn handle_event(
    app: &mut App,
    event: Event,
    terminal: &mut Terminal<CrosstermBackend<Tty>>,
) -> eyre::Result<()> {
    // Handle paste events.
    if let Event::Paste(text) = &event {
        if let Screen::Auth(AuthState::NeedLogin { input, error }) = &mut app.screen {
            input.push_str(text);
            *error = None;
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
    let Screen::Auth(state) = &mut app.screen else {
        return Ok(());
    };
    if let AuthState::NeedLogin { input, error } = state {
        match code {
            KeyCode::Char(c) => {
                input.push(c);
                *error = None;
            }
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Enter => {
                let key = input.trim().to_string();
                if key.is_empty() {
                    *error = Some("API key is required.".to_string());
                    return Ok(());
                }
                app.screen = Screen::Auth(AuthState::LoggingIn);
                terminal.draw(|f| super::render(f, app))?;
                match ops::login(&key).await {
                    Ok(_) => {
                        app.screen = Screen::Picker(PickerState {
                            error: None,
                            agents: Vec::new(),
                            selected: 0,
                            loading: true,
                        });
                        terminal.draw(|f| super::render(f, app))?;
                        picker::discover_agents(app).await;
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        let short = if msg.contains("invalid or expired") {
                            "Invalid API key.".to_string()
                        } else if msg.contains("failed to connect") {
                            "Could not connect to API.".to_string()
                        } else {
                            format!("Login failed: {msg}")
                        };
                        app.screen = Screen::Auth(AuthState::NeedLogin {
                            input: String::new(),
                            error: Some(short),
                        });
                    }
                }
            }
            KeyCode::Esc => {
                app.should_quit = true;
            }
            _ => {}
        }
    }
    Ok(())
}

fn centered_rect(width_pct: u16, height: u16, area: Rect) -> Rect {
    let popup_width = area.width * width_pct / 100;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, popup_width.min(area.width), height.min(area.height))
}
