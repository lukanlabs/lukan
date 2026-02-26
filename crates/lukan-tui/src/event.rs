use crossterm::event::{
    self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers, MouseEvent,
};
use std::time::Duration;
use tokio::sync::mpsc;

/// Application events (input + agent)
#[derive(Debug)]
pub enum AppEvent {
    /// A key was pressed
    Key(KeyEvent),
    /// Text pasted via bracketed paste
    Paste(String),
    /// Terminal was resized
    Resize(u16, u16),
    /// Mouse event
    Mouse(MouseEvent),
    /// Tick for UI updates
    Tick,
}

/// Spawn a background task that reads terminal events and sends them through a channel
pub fn spawn_event_reader(tx: mpsc::UnboundedSender<AppEvent>) {
    std::thread::spawn(move || {
        loop {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                match event::read() {
                    Ok(CrosstermEvent::Key(key)) => {
                        if tx.send(AppEvent::Key(key)).is_err() {
                            break;
                        }
                    }
                    Ok(CrosstermEvent::Paste(text)) => {
                        if tx.send(AppEvent::Paste(text)).is_err() {
                            break;
                        }
                    }
                    Ok(CrosstermEvent::Resize(w, h)) => {
                        if tx.send(AppEvent::Resize(w, h)).is_err() {
                            break;
                        }
                    }
                    Ok(CrosstermEvent::Mouse(mouse)) => {
                        if tx.send(AppEvent::Mouse(mouse)).is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            } else {
                // Send tick for animations/updates
                if tx.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        }
    });
}

/// Check if a key event is the quit shortcut (Ctrl+C or Ctrl+Q)
pub fn is_quit(key: &KeyEvent) -> bool {
    matches!(
        key,
        KeyEvent {
            code: KeyCode::Char('c') | KeyCode::Char('q'),
            modifiers: KeyModifiers::CONTROL,
            ..
        }
    )
}

/// Check if a key event is Enter/Return
pub fn is_submit(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Enter)
}
