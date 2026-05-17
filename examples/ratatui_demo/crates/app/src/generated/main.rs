// Module: main.r

use protocol::{AppState, Filter, Task};

use crate::generated::state::{createItem, filterByName};
#[derive(Debug, Clone)]
pub struct KeyEvent {
    pub code: String,
    pub char: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyCode {
    Char,
    Enter,
    Escape,
    Up,
    Down,
}

pub fn validate_price(price: f64) -> () {
    if (price < 0i32) {
                return () { ok: false, error: "Price cannot be negative" };
    }
    if (price > 1000000i32) {
                return () { ok: false, error: "Price exceeds maximum" };
    }
        return () { ok: true, value: price };
}

pub fn find_by_id(items: Vec<Item>, id: f64) -> Option<Item> {
    for i: i32 = 0i32; (i < items.len()); i += 1 {
        {
            if items.get(i).id == id {
                                return items.get(i);
            }
        }
    }
        return None;
}

pub fn update(state: &mut AppState) -> () {
    if state.items.len() == 0i32 {
        state.selected = 0i32;
    } else if (state.selected >= state.items.len()) {
        state.selected = state.items.len() - 1i32;
    }
    }
}

pub fn handle_key(key: KeyEvent, state: &mut AppState) -> () {
    match key.code {
        KeyCode::Down =>  {
        }
        KeyCode::Char =>  {
            if key.char == "j" {
                state.selected = std::cmp::min(state.selected + 1i32, state.items.len() - 1i32);
            }
        }
        KeyCode::Up =>  {
        }
        KeyCode::Char =>  {
            if key.char == "k" {
                state.selected = std::cmp::max(state.selected - 1i32, 0i32);
            }
        }
        KeyCode::Char =>  {
            if key.char == "a" {
                let item: () = create_item("New Item", 9.99_f64);
                state.items.push(item);
                state.selected = state.items.len() - 1i32;
            } else if key.char == "d" {
                if (state.items.len() > 0i32) {
                    let item: () = state.items.get(state.selected);
                    if item {
                        state.view = () { tag: "Detail", id: item.id };
                    }
                }
            } else if key.char == "e" {
                if (state.items.len() > 0i32) {
                    let item: () = state.items.get(state.selected);
                    if item {
                        state.view = () { tag: "Edit", id: item.id };
                    }
                }
            } else if key.char == "l" {
                state.view = () { tag: "List" };
            } else if key.char == "q" {
                state.shouldExit = true;
            }
            }
            }
            }
            }
        }
        KeyCode::Escape =>  {
            state.view = () { tag: "List" };
        }
    }
}


