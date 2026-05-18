//! # Host Binary
//!
//! Thin host binary that loads the app dylib and drives I/O.
//! The dylib mutates AppState. The host reads AppState and does I/O.

use std::path::PathBuf;
use libloading::{Library, Symbol};
use protocol::AppState;
use ratatui::{Terminal, backend::CrosstermBackend, widgets::{Block, Borders, List, ListItem, Paragraph, Clear}};

/// Dylib loader that manages library loading and symbol resolution.
pub struct DylibLoader {
    lib: Option<Library>,
    update_fn: Option<unsafe extern "C" fn(*mut AppState)>,
}

impl DylibLoader {
    /// Load a dylib from path.
    ///
    /// # Safety
    /// The path must point to a valid rune-generated dylib.
    #[allow(unsafe_code)]
    pub unsafe fn load(path: &PathBuf) -> Result<Self, libloading::Error> {
        let lib = Library::new(path)?;
        let update: Symbol<unsafe extern "C" fn(*mut AppState)> = lib.get(b"update\0")?;
        Ok(Self {
            lib: Some(lib),
            update_fn: Some(*update),
        })
    }

    /// Call the dylib's update function.
    #[allow(unsafe_code)]
    pub fn update(&self, state: &mut AppState) {
        if let Some(f) = self.update_fn {
            unsafe { f(state) };
        }
    }

    /// Check if the dylib is loaded.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.lib.is_some()
    }
}

/// Run the host event loop.
pub fn run_host() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = setup_terminal()?;
    let mut state = AppState::default();
    let mut loader: Option<DylibLoader> = None;
    let hot_dir = PathBuf::from("target/hot");
    let current_link = hot_dir.join(".current");

    loop {
        // Check for dylib reload
        loader = check_and_reload(&current_link, loader);

        // Poll input and handle directly in host
        if crossterm::event::poll(std::time::Duration::from_millis(50))? {
            if let Ok(crossterm::event::Event::Key(key)) = crossterm::event::read() {
                match key.code {
                    crossterm::event::KeyCode::Char('q') => state.should_exit = true,
                    crossterm::event::KeyCode::Char('j') | crossterm::event::KeyCode::Down => {
                        state.selected = (state.selected + 1).min(state.tasks.len().saturating_sub(1));
                    }
                    crossterm::event::KeyCode::Char('k') | crossterm::event::KeyCode::Up => {
                        state.selected = state.selected.saturating_sub(1);
                    }
                    crossterm::event::KeyCode::Char('x') => {
                        if let Some(task) = state.tasks.get_mut(state.selected) {
                            task.done = !task.done;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Run dylib update
        if let Some(ref l) = loader {
            l.update(&mut state);
        }

        // Render from state every frame
        let _ = terminal.draw(|f| {
            render_state(f, &state);
        });

        if state.should_exit {
            break;
        }
    }

    cleanup_terminal();
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>, Box<dyn std::error::Error>> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

#[allow(unsafe_code)]
fn check_and_reload(
    current_link: &PathBuf,
    loader: Option<DylibLoader>,
) -> Option<DylibLoader> {
    if !current_link.exists() {
        return loader;
    }

    if let Ok(target) = std::fs::read_link(current_link) {
        let needs_reload = loader
            .as_ref()
            .and_then(|l| l.lib.as_ref())
            .map(|lib| lib.path() != Some(target.clone()))
            .unwrap_or(true);

        if needs_reload {
            if let Ok(new_loader) = unsafe { DylibLoader::load(&target) } {
                return Some(new_loader);
            }
        }
    }

    loader
}

fn convert_key_event(key: crossterm::event::KeyEvent) -> Option<KeyEvent> {
    use crossterm::event::KeyCode;
    match key.code {
        KeyCode::Char(c) => Some(KeyEvent::Char(c)),
        KeyCode::Up => Some(KeyEvent::Up),
        KeyCode::Down => Some(KeyEvent::Down),
        KeyCode::Left => Some(KeyEvent::Left),
        KeyCode::Right => Some(KeyEvent::Right),
        KeyCode::Enter => Some(KeyEvent::Enter),
        KeyCode::Esc => Some(KeyEvent::Esc),
        KeyCode::Backspace => Some(KeyEvent::Backspace),
        KeyCode::Tab => Some(KeyEvent::Tab),
        KeyCode::Delete => Some(KeyEvent::Delete),
        KeyCode::Home => Some(KeyEvent::Home),
        KeyCode::End => Some(KeyEvent::End),
        KeyCode::PageUp => Some(KeyEvent::PageUp),
        KeyCode::PageDown => Some(KeyEvent::PageDown),
        _ => Some(KeyEvent::Other),
    }
}

fn render_state(frame: &mut ratatui::Frame, state: &AppState) {
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span, Text};

    let area = frame.size();

    // Draw border block
    let block = Block::default()
        .title("Rune Todos")
        .borders(Borders::ALL);
    frame.render_widget(block, area);

    // Build task list lines
    let inner = Rect::new(area.x + 2, area.y + 1, area.width - 4, area.height - 2);
    let mut lines: Vec<Line> = Vec::new();

    for (i, task) in state.tasks.iter().enumerate() {
        let marker = if task.done { "[x]" } else { "[ ]" };
        let prefix = if i == state.selected { "> " } else { "  " };
        let text = format!("{}{} {}", prefix, marker, task.title);
        let style = if i == state.selected {
            Style::default().bg(Color::Blue)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(text, style)));
    }

    if state.tasks.is_empty() {
        lines.push(Line::from("No tasks yet. Add some!"));
    }

    lines.push(Line::from(""));
    lines.push(Line::from("j/k: navigate | x: toggle | q: quit"));

    let text_widget = ratatui::widgets::Paragraph::new(Text::from(lines));
    frame.render_widget(text_widget, inner);
}

fn cleanup_terminal() {
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen
    );
    let _ = crossterm::terminal::disable_raw_mode();
}
