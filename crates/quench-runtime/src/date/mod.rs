//! Date builtin implementation using chrono.
//!
//! ECMAScript dates are represented as milliseconds since the Unix epoch,
//! with special handling for NaN (invalid dates) and TimeClip limits.

pub(crate) mod chrono_utils;
mod format;
mod helpers;
mod impl_;
mod setter;

use std::rc::Rc;

use crate::value::Value;

pub fn set_mock_now(value: Option<f64>) {
    chrono_utils::set_mock_now(value);
}
pub fn set_local_timezone(name: Option<&str>) {
    chrono_utils::set_local_timezone(name);
}
pub fn current_time_ms() -> f64 {
    chrono_utils::current_time_ms()
}
pub fn mock_enabled() -> bool {
    chrono_utils::mock_enabled()
}

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
                Value::Number(ms) => Some(ms),
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
    Value::BindingCell(crate::value::BindingCell::new(Value::Number(ms)))
}

/// Construct a Date instance for host APIs that need to return a timestamp.
pub fn instance(ms: f64) -> Value {
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("timeValue".to_string(), time_property(ms)),
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::DatePrototype),
        ),
    ])))
}

pub fn local_tz_offset_minutes() -> i32 {
    chrono_utils::local_tz_offset_minutes()
}

pub fn local_components(ms: f64) -> Option<(i32, u32, u32, u32, u32, u32, u32)> {
    chrono_utils::local_components(ms)
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
