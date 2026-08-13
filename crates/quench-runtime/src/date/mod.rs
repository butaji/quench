//! Date builtin implementation using chrono.
//!
//! ECMAScript dates are represented as milliseconds since the Unix epoch,
//! with special handling for NaN (invalid dates) and TimeClip limits.

mod chrono_utils;
mod helpers;
mod impl_;

use std::{cell::RefCell, rc::Rc};

use crate::value::Value;

/// Internal Date representation: milliseconds since Unix epoch.
#[derive(Debug, Clone, PartialEq)]
pub struct DateValue {
    pub ms: f64,
}

impl DateValue {
    pub fn new(ms: f64) -> Self {
        Self {
            ms: chrono_utils::time_clip(ms),
        }
    }

    pub fn now() -> Self {
        Self {
            ms: chrono_utils::current_time_ms(),
        }
    }

    pub fn utc(y: f64, m: f64, d: f64, h: f64, min: f64, s: f64, ms: f64) -> Self {
        let year = chrono_utils::normalize_constructor_year(y);
        let ms = chrono_utils::make_date_ms(year, m, d, h, min, s, ms);
        Self { ms }
    }

    pub fn parse(s: &str) -> f64 {
        chrono_utils::parse_date_string(s)
    }
}

/// Extract time value (ms since epoch) from a Date Value.
pub fn extract_time(receiver: Option<&Value>) -> f64 {
    match receiver {
        Some(Value::Object(props)) => props
            .iter()
            .find(|(k, _)| k == "timeValue")
            .and_then(|(_, v)| match v {
                Value::Number(ms) => Some(*ms),
                Value::BindingCell(cell) => match &*cell.borrow() {
                    Value::Number(ms) => Some(*ms),
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

pub fn time_property(ms: f64) -> Value {
    Value::BindingCell(Rc::new(RefCell::new(Value::Number(ms))))
}

/// Store time value in a Date Object.
pub fn store_time(receiver: &Value, ms: f64) -> Value {
    crate::builtins::set_property(
        receiver.clone(),
        "timeValue",
        Value::Number(chrono_utils::time_clip(ms)),
    )
}

pub(crate) use impl_::call;
pub use impl_::execute;
