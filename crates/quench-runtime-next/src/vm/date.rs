use super::*;
use crate::host::{CapabilityId, HostContext};
use chrono::{DateTime, Duration, TimeZone, Utc};

impl<H: Host> Vm<H> {
    pub(super) fn install_date(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let date = self.native_value(Native::Date);
        let prototype = self.object();
        self.set_named(program, date, "prototype", prototype)?;
        for (name, native) in [
            ("now", Native::DateNow),
            ("parse", Native::DateParse),
            ("UTC", Native::DateUTC),
        ] {
            self.set_named(program, date, name, self.native_value(native))?;
        }
        self.global(program, "Date", date)
    }

    pub(super) fn date_static_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::DateParse => {
                let text = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let millis = chrono::DateTime::parse_from_rfc3339(&text)
                    .ok()
                    .map(|date| date.timestamp_millis() as f64)
                    .unwrap_or(f64::NAN);
                Ok(Value::number(millis))
            }
            Native::DateUTC => {
                let year = date_component(self, p, args, 0, 0.0)? as i32;
                let month = date_component(self, p, args, 1, 0.0)? as u32;
                let day = date_component(self, p, args, 2, 1.0)? as u32;
                let hour = date_component(self, p, args, 3, 0.0)? as u32;
                let minute = date_component(self, p, args, 4, 0.0)? as u32;
                let second = date_component(self, p, args, 5, 0.0)? as u32;
                let millis = date_component(self, p, args, 6, 0.0)? as u32;
                let value = Utc
                    .with_ymd_and_hms(year, month + 1, day, hour, minute, second)
                    .single()
                    .map(|date| date.timestamp_millis() as f64 + f64::from(millis))
                    .unwrap_or(f64::NAN);
                Ok(Value::number(value))
            }
            _ => Err(JsError("invalid static Date native".into())),
        }
    }

    pub(super) fn date_property_native(&self, atom: Atom) -> Value {
        [
            ("getTime", Native::DateGetTime),
            ("valueOf", Native::DateValueOf),
            ("getTimezoneOffset", Native::DateGetTimezoneOffset),
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
        let Some(Cell::Date { milliseconds, .. }) = self.heap.get(this) else {
            return Err(JsError(
                "Date method called on incompatible receiver".into(),
            ));
        };
        let milliseconds = *milliseconds;
        match native {
            Native::DateGetTime | Native::DateValueOf => Ok(Value::number(milliseconds)),
            Native::DateGetTimezoneOffset => Ok(Value::number(0.0)),
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
                Ok(self.heap.alloc(Cell::String(text.into())))
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

    pub(super) fn date_construct_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let milliseconds = if args.len() < 2 {
            match args.first().copied() {
                None => HostContext::new(&mut self.host).invoke(CapabilityId::ClockMillis, None),
                Some(value) => self.to_number(p, value)?,
            }
        } else {
            let mut parts = [0.0; 7];
            parts[2] = 1.0;
            for (index, value) in args.iter().take(7).enumerate() {
                parts[index] = self.to_number(p, *value)?;
            }
            let mut year = if (0.0..=99.0).contains(&parts[0]) {
                parts[0] as i32 + 1900
            } else {
                parts[0] as i32
            };
            let month = parts[1] as i32;
            year += month.div_euclid(12);
            let month = month.rem_euclid(12) as u32 + 1;
            Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0)
                .single()
                .map(|date| {
                    let day = parts[2].trunc() as i64 - 1;
                    let time = parts[3].trunc() as i64 * 3_600_000
                        + parts[4].trunc() as i64 * 60_000
                        + parts[5].trunc() as i64 * 1_000
                        + parts[6].trunc() as i64;
                    (date + Duration::days(day) + Duration::milliseconds(time)).timestamp_millis()
                        as f64
                })
                .unwrap_or(f64::NAN)
        };
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self
            .own_property(self.native_value(Native::Date), prototype_atom)
            .unwrap_or(self.object_proto);
        Ok(self.heap.alloc(Cell::Date {
            milliseconds,
            object: Box::new(Self::empty_object(prototype)),
        }))
    }
}

fn date_component<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    args: &[Value],
    index: usize,
    default: f64,
) -> Result<f64, JsError> {
    args.get(index)
        .copied()
        .map_or(Ok(default), |value| vm.to_number(p, value))
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
