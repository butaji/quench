//! Generated from views/task_list.r.tsx

use protocol::{AppState, Filter, Task};
use ratatui::{
    widgets::{Block, Borders, List, ListItem, Paragraph},
    style::{Style, Color},
    widgets::Widget,
};

/// Task list component.
pub fn render(state: &AppState) -> Box<dyn Widget> {
    let visible_tasks: Vec<&Task> = match state.filter {
        Filter::Active => state.tasks.iter().filter(|t| !t.done).collect(),
        Filter::Completed => state.tasks.iter().filter(|t| t.done).collect(),
        Filter::All => state.tasks.iter().collect(),
    };

    let items: Vec<ListItem> = visible_tasks.iter().enumerate().map(|(i, task)| {
        let checkbox = if task.done { "[x]" } else { "[ ]" };
        let content = format!("{} {}", checkbox, task.title);
        let is_selected = i == state.selected;
        let style = if is_selected {
            Style::new().add_modifier(ratatui::style::Modifier::REVERSED)
        } else if task.done {
            Style::new().fg(Color::DarkGray)
        } else {
            Style::default()
        };
        ListItem::new(content).style(style)
    }).collect();

    if visible_tasks.is_empty() {
        Box::new(Paragraph::new("No tasks. Press 'a' to add one.")
            .block(Block::default().borders(Borders::ALL)))
    } else {
        Box::new(List::new(items)
            .block(Block::default()
                .title(format!("Tasks ({})", visible_tasks.len()))
                .borders(Borders::ALL))
            .highlight_style(Style::new().bg(Color::Blue)))
    }
}
