//! Generated from views/root.r.tsx

use protocol::{AppState, Task};
use ratatui::{
    widgets::{Block, Borders, List, ListItem, Paragraph},
    style::Style,
    widgets::Widget,
};

/// Main root view component.
pub fn render(state: &AppState) -> impl Widget {
    let items: Vec<ListItem> = state.tasks.iter().enumerate().map(|(i, task)| {
        let checkbox = if task.done { "[x]" } else { "[ ]" };
        let title = if task.done {
            task.title.chars().map(|c| format!("{}\u{0336}", c)).collect::<String>()
        } else {
            task.title.clone()
        };
        let content = format!("{} {}", checkbox, title);
        let style = if i == state.selected {
            Style::new().fg(ratatui::style::Color::Yellow)
        } else {
            Style::default()
        };
        ListItem::new(content).style(style)
    }).collect();

    List::new(items)
        .block(Block::default()
            .title("TODOX")
            .borders(Borders::ALL))
        .highlight_style(Style::new().bg(ratatui::style::Color::DarkGray))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render() {
        let state = AppState {
            tasks: vec![
                Task { id: 1, title: "Test 1".to_string(), done: false },
                Task { id: 2, title: "Test 2".to_string(), done: true },
            ],
            selected: 0,
            filter: protocol::Filter::All,
            should_exit: false,
        };

        let _widget = render(&state);
        // Widget is created successfully
    }
}
