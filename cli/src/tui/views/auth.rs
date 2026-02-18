use crate::tui::Tty;
use crate::tui::app::{App, AuthState, PickerState, Screen};
use crate::tui::ops;
use crate::tui::views::picker;
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn render(f: &mut Frame, state: &AuthState) {
    let area = centered_rect(60, 12, f.area());
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
                Line::from(Span::styled(
                    "Sign up: https://asterai.io/login",
                    Style::default().fg(Color::Cyan),
                )),
                Line::from(""),
            ];
            if let Some(err) = error {
                lines.push(Line::from(Span::styled(
                    err.as_str(),
                    Style::default().fg(Color::Red),
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
    let Event::Key(KeyEvent { code, .. }) = event else {
        return Ok(());
    };
    let Screen::Auth(state) = &mut app.screen else {
        return Ok(());
    };
    match state {
        AuthState::NeedLogin { input, error } => match code {
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
                            agents: Vec::new(),
                            selected: 0,
                            loading: true,
                        });
                        terminal.draw(|f| super::render(f, app))?;
                        picker::discover_agents(app).await;
                    }
                    Err(e) => {
                        app.screen = Screen::Auth(AuthState::NeedLogin {
                            input: String::new(),
                            error: Some(format!("Login failed: {e}")),
                        });
                    }
                }
            }
            KeyCode::Esc => {
                app.should_quit = true;
            }
            _ => {}
        },
        _ => {}
    }
    Ok(())
}

fn centered_rect(width_pct: u16, height: u16, area: Rect) -> Rect {
    let popup_width = area.width * width_pct / 100;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, popup_width.min(area.width), height.min(area.height))
}
