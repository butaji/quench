// Module: main.r

use protocol::{AppState, Filter, Task};
use ratatui::widgets::{Widget, Paragraph, ListItem, Span};
use ratatui::style::Style;
use crossterm::event::KeyCode;
use serde_json;

use crate::native;

pub fn update(state: AppState) -> () {
    let filtered: () = filter_tasks(state.tasks, state.filter);
    if state.selected >= filtered.len() && filtered.len() > 0i32 {
        ();
    }
}

pub fn filter_tasks(tasks: Vec<Task>, filter: Filter) -> Vec<Task> {
    // switch
}

pub fn get_stats(tasks: Vec<Task>) -> __AnonymousStruct1 {
    let done: usize = tasks.filter(|t| t.done).len();
    return { total: tasks.len(), done: done, active: tasks.len() - done };
}

pub fn find_task(tasks: Vec<Task>, id: f64) -> Option<Task> {
    // unsupported
    return None;
}

pub fn sort_tasks(tasks: Vec<Task>, by: ()) -> Vec<Task> {
    let sorted: Vec<()> = vec![tasks];
    // switch
}

#[derive(Debug, Clone)]
pub struct __AnonymousStruct1 {
pub total: f64,
pub done: f64,
pub active: f64,
}

