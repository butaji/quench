//! Protocol crate for std_library example.

use serde::{Deserialize, Serialize};

/// Application state that survives hot reloads.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppState {
    pub data: Vec<DataEntry>,
    pub search_query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataEntry {
    pub id: String,
    pub name: String,
    pub value: f64,
    pub timestamp: i64,
}
