//! # App Templates
//!
//! Templates for the app crate.

/// lib.rs template for generated app.
pub const APP_LIB: &str = r#"//! Generated app library

mod native;

pub mod generated;

use serde::{Deserialize, Serialize};

/// Application state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppState {
    pub tasks: Vec<Task>,
    pub selected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: i32,
    pub title: String,
    pub done: bool,
}

/// Host signal handler.
#[no_mangle]
pub extern "C" fn update() {
    generated::main::update();
}
"#;

/// Native module template.
pub const NATIVE_MOD: &str = r#"//! Native Rust helpers available to runie code.

/// Fast math utilities.
pub fn fast_add(a: i32, b: i32) -> i32 {
    a + b
}
"#;
