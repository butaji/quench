//! Native Rust utilities for data processing

use serde::{Deserialize, Serialize};

/// A batch of data for processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch<T> {
    pub items: Vec<T>,
    pub timestamp: i64,
    pub batch_id: u64,
}

impl<T> Batch<T> {
    /// Create a new batch
    pub fn new(items: Vec<T>, batch_id: u64) -> Self {
        Self {
            items,
            timestamp: chrono::Utc::now().timestamp(),
            batch_id,
        }
    }

    /// Get batch size
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if batch is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Statistics for numeric data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumericStats {
    pub sum: f64,
    pub mean: f64,
    pub variance: f64,
    pub std_dev: f64,
}

impl NumericStats {
    /// Calculate statistics from values
    pub fn calculate(values: &[f64]) -> Self {
        if values.is_empty() {
            return Self {
                sum: 0.0,
                mean: 0.0,
                variance: 0.0,
                std_dev: 0.0,
            };
        }

        let sum: f64 = values.iter().sum();
        let mean = sum / values.len() as f64;
        let variance = if values.len() > 1 {
            values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64
        } else {
            0.0
        };
        let std_dev = variance.sqrt();

        Self {
            sum,
            mean,
            variance,
            std_dev,
        }
    }
}
