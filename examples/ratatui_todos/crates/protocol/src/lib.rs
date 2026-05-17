//! Protocol crate - shared state contract between host and app.
//!
//! This module defines the AppState trait and AppStateData that
//! must be implemented by the app and understood by the host.

use serde::{Deserialize, Serialize};

/// Application state that survives dylib hot reloads.
/// The host serializes this before reload and deserializes after.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStateData {
    /// Serialized app state as JSON
    pub state_json: String,
}

/// Trait for application state operations.
/// Implemented by the app's state module.
pub trait AppStateTrait: Serialize + for<'de> Deserialize<'de> {
    /// Create initial state
    fn new() -> Self;

    /// Serializes state to JSON for transfer
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Deserializes state from JSON
    fn from_json(json: &str) -> Option<Self>
    where
        Self: Sized,
    {
        serde_json::from_str(json).ok()
    }
}
