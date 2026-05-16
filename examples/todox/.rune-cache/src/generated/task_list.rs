// Module: task_list.r

use protocol::{AppState, Filter, Task};
use ratatui::widgets::{Widget, Paragraph, ListItem, Span};
use ratatui::style::Style;
use crossterm::event::KeyCode;
use serde_json;

use crate::native;

#[derive(Debug, Clone)]
pub struct TaskListProps {
pub tasks: Vec<Task>,
pub selected_index: f64,
pub show_completed: bool,
}

pub fn task_list(props: TaskListProps) -> Widget {
    let visible_tasks: () = if props.show_completed { props.tasks } else { props.tasks.filter(|t| !t.done) };
    return (());
}

pub fn task_row(props: __AnonymousStruct1) -> ListItem {
    let checkbox: String = if props.task.done { "[x]" } else { "[ ]" };
    let title: () = if props.task.done { strikethrough(props.task.title) } else { props.task.title };
    return (());
}

pub fn empty_state() -> Paragraph {
    return (());
}

pub fn strikethrough(text: String) -> String {
    return text.split("").join("̶");
}

pub fn filter_tabs(props: __AnonymousStruct2) -> Widget {
    let tabs: Vec<()> = vec!["All", "Active", "Completed"];
    return (());
}

pub fn tab_button(props: __AnonymousStruct3) -> Span {
    let prefix: String = if props.active { "> " } else { "  " };
    return ();
}

#[derive(Debug, Clone)]
pub struct __AnonymousStruct1 {
pub task: Task,
pub is_selected: bool,
}

#[derive(Debug, Clone)]
pub struct __AnonymousStruct2 {
pub current: String,
pub on_select: (),
}

#[derive(Debug, Clone)]
pub struct __AnonymousStruct3 {
pub label: String,
pub active: bool,
pub on_click: (),
}

