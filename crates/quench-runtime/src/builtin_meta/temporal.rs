use crate::ops::Builtin;

pub const fn fn_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::TemporalDuration => Some("Temporal.Duration"),
        Builtin::TemporalDurationFrom => Some("Temporal.Duration.from"),
        Builtin::TemporalDurationCompare => Some("Temporal.Duration.compare"),
        Builtin::TemporalPlainDate => Some("Temporal.PlainDate"),
        Builtin::TemporalPlainDateFrom => Some("Temporal.PlainDate.from"),
        _ => None,
    }
}

pub const fn short_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::TemporalDuration => Some("Duration"),
        Builtin::TemporalDurationFrom => Some("from"),
        Builtin::TemporalDurationCompare => Some("compare"),
        Builtin::TemporalPlainDate => Some("PlainDate"),
        Builtin::TemporalPlainDateFrom => Some("from"),
        _ => None,
    }
}

pub const fn fn_len(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::TemporalDurationFrom => Some(1.0),
        Builtin::TemporalDurationCompare => Some(2.0),
        Builtin::TemporalPlainDateFrom => Some(1.0),
        _ => None,
    }
}
