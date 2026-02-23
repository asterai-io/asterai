use crate::tui::Tty;
use crate::tui::app::{App, Screen};
use crossterm::event::Event;
use ratatui::prelude::*;

pub mod auth;
pub mod chat;
pub mod picker;
pub mod setup;

pub fn render(f: &mut Frame, app: &App) {
    match &app.screen {
        Screen::Auth(state) => auth::render(f, state),
        Screen::Picker(state) => picker::render(f, state, app),
        Screen::Setup(state) => setup::render(f, state),
        Screen::Chat(state) => chat::render(f, state, app),
    }
}

pub async fn handle_event(
    app: &mut App,
    event: Event,
    terminal: &mut Terminal<CrosstermBackend<Tty>>,
) -> eyre::Result<()> {
    match &app.screen {
        Screen::Auth(_) => auth::handle_event(app, event, terminal).await,
        Screen::Picker(_) => picker::handle_event(app, event, terminal).await,
        Screen::Setup(_) => setup::handle_event(app, event, terminal).await,
        Screen::Chat(_) => chat::handle_event(app, event).await,
    }
}
