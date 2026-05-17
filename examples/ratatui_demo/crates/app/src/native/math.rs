//! Native Rust utilities for the app

/// Calculate percentage of a value
pub fn percentage(value: f64, total: f64) -> f64 {
    if total == 0.0 {
        return 0.0;
    }
    (value / total) * 100.0
}

/// Format currency for display
pub fn format_currency(amount: f64) -> String {
    format!("${:.2}", amount)
}

/// Clamp a value between min and max
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}
