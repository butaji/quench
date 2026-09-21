use super::*;
use chrono::{DateTime, TimeZone, Utc};

impl<H: Host> Vm<H> {
    pub(super) fn date_property_native(&self, atom: Atom) -> Value {
        [
            ("getTime", Native::DateGetTime),
            ("valueOf", Native::DateValueOf),
            ("toISOString", Native::DateToISOString),
            ("toJSON", Native::DateToJSON),
        ]
        .into_iter()
        .find_map(|(name, native)| {
            (self.lookup_atom(name) == Some(atom)).then(|| self.native_value(native))
        })
        .unwrap_or(Value::UNDEFINED)
    }

    pub(super) fn date_native(&mut self, native: Native, this: Value) -> Result<Value, JsError> {
        let Some(Cell::Date(milliseconds)) = self.heap.get(this) else {
            return Err(JsError(
                "Date method called on incompatible receiver".into(),
            ));
        };
        let milliseconds = *milliseconds;
        match native {
            Native::DateGetTime | Native::DateValueOf => Ok(Value::number(milliseconds)),
            Native::DateToISOString | Native::DateToJSON => {
                if !milliseconds.is_finite() {
                    return Err(JsError("Invalid time value".into()));
                }
                let millis = milliseconds.trunc();
                let date = Utc
                    .timestamp_millis_opt(millis as i64)
                    .single()
                    .ok_or_else(|| JsError("Invalid time value".into()))?;
                let text = format_date(date);
                Ok(self.heap.alloc(Cell::String(text)))
            }
            _ => Err(JsError("invalid Date native".into())),
        }
    }

    pub(super) fn date_to_json_string(&self, milliseconds: f64) -> Option<String> {
        if !milliseconds.is_finite() {
            return None;
        }
        let date = Utc
            .timestamp_millis_opt(milliseconds.trunc() as i64)
            .single()?;
        Some(format_date(date))
    }
}

fn format_date(date: DateTime<Utc>) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        date.year(),
        date.month(),
        date.day(),
        date.hour(),
        date.minute(),
        date.second(),
        date.timestamp_subsec_millis(),
    )
}

use chrono::{Datelike, Timelike};
