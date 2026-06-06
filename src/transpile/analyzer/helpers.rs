//! Analyzer helper functions

/// Check if a name is a hook (starts with "use" and length > 3)
pub fn is_hook_name(name: &str) -> bool {
    name.starts_with("use") && name.len() > 3
}

/// Check if a name is a signal-related identifier
pub fn is_signal_name(name: &str) -> bool {
    name == "signal"
        || name.starts_with("signal")
        || name.starts_with("useSignal")
        || name.starts_with("useComputed")
}
