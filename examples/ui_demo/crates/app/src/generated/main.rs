// Module: main.r

use protocol::{AppState, Filter, Task};

#[derive(Debug, Clone)]
pub struct State {
    pub counter: f64,
    pub items: Vec<String>,
    pub selected_index: f64,
    pub input_buffer: String,
}

pub fn create_initial_state() -> State {
        return State { counter: 0i32, items: vec!["Learn Rust", "Build UI", "Ship product"] as Vec<String>, selectedIndex: 0i32, inputBuffer: "" };
}

pub fn increment_counter(state: State) -> () {
    state.counter = state.counter + 1i32;
}

pub fn decrement_counter(state: State) -> () {
    state.counter = state.counter - 1i32;
}

pub fn move_selection_up(state: State) -> () {
    if (state.selectedIndex > 0i32) {
        state.selectedIndex = state.selectedIndex - 1i32;
    }
}

pub fn move_selection_down(state: State) -> () {
    if (state.selectedIndex < state.items.len() - 1i32) {
        state.selectedIndex = state.selectedIndex + 1i32;
    }
}

pub fn add_item(state: State, item: String) -> () {
    state.items.push(item);
    state.selectedIndex = state.items.len() - 1i32;
}

pub fn remove_selected(state: State) -> () {
    if (state.items.len() > 0i32) && (state.selectedIndex < state.items.len()) {
        state.items.splice(state.selectedIndex..(state.selectedIndex + 1i32), vec![]);
        if (state.selectedIndex >= state.items.len()) && (state.items.len() > 0i32) {
            state.selectedIndex = state.items.len() - 1i32;
        }
    }
}

pub fn toggle_item(state: State) -> () {
    move_selection_down(state);
}


