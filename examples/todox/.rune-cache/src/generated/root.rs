// Module: root.r

use protocol::{AppState, Filter, Task};
use ratatui::widgets::{Widget, Paragraph, ListItem, Span};
use ratatui::style::Style;
use crossterm::event::KeyCode;
use serde_json;

use crate::native;

#[derive(Debug, Clone)]
pub struct RootViewProps {
pub tasks: Vec<Task>,
pub selected: f64,
pub filter: String,
}

pub fn root_view(props: RootViewProps) -> Widget {
    return (());
}

pub fn checkbox(props: __AnonymousStruct1) -> String {
    return if props.checked { "[x]" } else { "[ ]" };
}

pub fn footer(props: __AnonymousStruct2) -> Paragraph {
    let done: usize = props.tasks.filter(|t| t.done).len();
    let total: usize = props.tasks.length;
    return (());
}

pub fn progress_bar(props: __AnonymousStruct3) -> Widget {
    let values: () = props.tasks.map(|t| (if t.done { 1i32 } else { 0i32 }));
    return (());
}

pub fn task_item(props: __AnonymousStruct4) -> ListItem {
    return (());
}

#[derive(Debug, Clone)]
pub struct __AnonymousStruct1 {
pub checked: bool,
}

#[derive(Debug, Clone)]
pub struct __AnonymousStruct2 {
pub tasks: Vec<Task>,
}

#[derive(Debug, Clone)]
pub struct __AnonymousStruct3 {
pub tasks: Vec<Task>,
}

#[derive(Debug, Clone)]
pub struct __AnonymousStruct4 {
pub task: Task,
pub selected: bool,
}

