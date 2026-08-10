//! Date method metadata.

use crate::ops::Builtin;

/// Returns the builtin for a DatePrototype property key.
pub fn date_prop(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "toString" => Some(DateToString),
        "valueOf" => Some(DateValueOf),
        "getTime" => Some(DateGetTime),
        "getFullYear" => Some(DateGetFullYear),
        "getMonth" => Some(DateGetMonth),
        "getDate" => Some(DateGetDate),
        "getDay" => Some(DateGetDay),
        "getHours" => Some(DateGetHours),
        "getMinutes" => Some(DateGetMinutes),
        "getSeconds" => Some(DateGetSeconds),
        "getMilliseconds" => Some(DateGetMilliseconds),
        "getTimezoneOffset" => Some(DateGetTimezoneOffset),
        _ => date_prop_utc(key),
    }
}

fn date_prop_utc(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "getUTCFullYear" => Some(DateGetUTCFullYear),
        "getUTCMonth" => Some(DateGetUTCMonth),
        "getUTCDate" => Some(DateGetUTCDate),
        "getUTCDay" => Some(DateGetUTCDay),
        "getUTCHours" => Some(DateGetUTCHours),
        "getUTCMinutes" => Some(DateGetUTCMinutes),
        "getUTCSeconds" => Some(DateGetUTCSeconds),
        "getUTCMilliseconds" => Some(DateGetUTCMilliseconds),
        _ => date_prop_setter(key),
    }
}

fn date_prop_setter(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "setTime" => Some(DateSetTime),
        "setFullYear" => Some(DateSetFullYear),
        "setMonth" => Some(DateSetMonth),
        "setDate" => Some(DateSetDate),
        "setHours" => Some(DateSetHours),
        "setMinutes" => Some(DateSetMinutes),
        "setSeconds" => Some(DateSetSeconds),
        "setMilliseconds" => Some(DateSetMilliseconds),
        "setUTCHours" => Some(DateSetUTCHours),
        "setUTCMinutes" => Some(DateSetUTCMinutes),
        "setUTCSeconds" => Some(DateSetUTCSeconds),
        "setUTCMilliseconds" => Some(DateSetUTCMilliseconds),
        "getYear" => Some(DateGetYear),
        "setYear" => Some(DateSetYear),
        "toLocaleString" => Some(DateToLocaleString),
        "toLocaleDateString" => Some(DateToLocaleDateString),
        "toLocaleTimeString" => Some(DateToLocaleTimeString),
        _ => None,
    }
}

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    use Builtin::*;
    match b {
        DateNow => Some("Date.now"),
        DateParse => Some("Date.parse"),
        DateUTC => Some("Date.UTC"),
        DateToString => Some("Date.prototype.toString"),
        DateValueOf => Some("Date.prototype.valueOf"),
        DateGetTime => Some("Date.prototype.getTime"),
        DateGetFullYear => Some("Date.prototype.getFullYear"),
        DateGetMonth => Some("Date.prototype.getMonth"),
        DateGetDate => Some("Date.prototype.getDate"),
        DateGetDay => Some("Date.prototype.getDay"),
        DateGetHours => Some("Date.prototype.getHours"),
        DateGetMinutes => Some("Date.prototype.getMinutes"),
        DateGetSeconds => Some("Date.prototype.getSeconds"),
        DateGetMilliseconds => Some("Date.prototype.getMilliseconds"),
        DateGetTimezoneOffset => Some("Date.prototype.getTimezoneOffset"),
        _ => fn_name_utc(b),
    }
}

const fn fn_name_utc(b: Builtin) -> Option<&'static str> {
    use Builtin::*;
    match b {
        DateGetUTCFullYear => Some("Date.prototype.getUTCFullYear"),
        DateGetUTCMonth => Some("Date.prototype.getUTCMonth"),
        DateGetUTCDate => Some("Date.prototype.getUTCDate"),
        DateGetUTCDay => Some("Date.prototype.getUTCDay"),
        DateGetUTCHours => Some("Date.prototype.getUTCHours"),
        DateGetUTCMinutes => Some("Date.prototype.getUTCMinutes"),
        DateGetUTCSeconds => Some("Date.prototype.getUTCSeconds"),
        DateGetUTCMilliseconds => Some("Date.prototype.getUTCMilliseconds"),
        _ => fn_name_set(b),
    }
}

const fn fn_name_set(b: Builtin) -> Option<&'static str> {
    use Builtin::*;
    match b {
        DateSetTime => Some("Date.prototype.setTime"),
        DateSetFullYear => Some("Date.prototype.setFullYear"),
        DateSetMonth => Some("Date.prototype.setMonth"),
        DateSetDate => Some("Date.prototype.setDate"),
        DateSetHours => Some("Date.prototype.setHours"),
        DateSetMinutes => Some("Date.prototype.setMinutes"),
        DateSetSeconds => Some("Date.prototype.setSeconds"),
        DateSetMilliseconds => Some("Date.prototype.setMilliseconds"),
        DateSetUTCHours => Some("Date.prototype.setUTCHours"),
        DateSetUTCMinutes => Some("Date.prototype.setUTCMinutes"),
        DateSetUTCSeconds => Some("Date.prototype.setUTCSeconds"),
        DateSetUTCMilliseconds => Some("Date.prototype.setUTCMilliseconds"),
        DateGetYear => Some("Date.prototype.getYear"),
        DateSetYear => Some("Date.prototype.setYear"),
        DateToLocaleString => Some("Date.prototype.toLocaleString"),
        DateToLocaleDateString => Some("Date.prototype.toLocaleDateString"),
        DateToLocaleTimeString => Some("Date.prototype.toLocaleTimeString"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    use Builtin::*;
    match b {
        DateNow => Some(0.0),
        DateParse => Some(1.0),
        DateUTC => Some(7.0),
        _ => fn_len_get_or_set(b),
    }
}

const fn fn_len_get_or_set(b: Builtin) -> Option<f64> {
    use Builtin::*;
    match b {
        DateGetTime
        | DateGetFullYear
        | DateGetMonth
        | DateGetDate
        | DateGetDay
        | DateGetHours
        | DateGetMinutes
        | DateGetSeconds
        | DateGetMilliseconds
        | DateGetTimezoneOffset
        | DateGetUTCFullYear
        | DateGetUTCMonth
        | DateGetUTCDate
        | DateGetUTCDay
        | DateGetUTCHours
        | DateGetUTCMinutes
        | DateGetUTCSeconds
        | DateGetUTCMilliseconds
        | DateValueOf
        | DateGetYear
        | DateToLocaleString
        | DateToLocaleDateString
        | DateToLocaleTimeString => Some(0.0),
        DateSetTime
        | DateSetMonth
        | DateSetDate
        | DateSetMilliseconds
        | DateSetYear
        | DateSetUTCMilliseconds => Some(1.0),
        DateSetFullYear => Some(4.0),
        DateSetHours | DateSetUTCHours => Some(5.0),
        DateSetMinutes | DateSetUTCMinutes => Some(3.0),
        DateSetSeconds | DateSetUTCSeconds => Some(2.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    use Builtin::*;
    match b {
        DateNow => Some("now"),
        DateParse => Some("parse"),
        DateUTC => Some("UTC"),
        DateToString => Some("toString"),
        DateValueOf => Some("valueOf"),
        DateGetTime => Some("getTime"),
        DateGetFullYear => Some("getFullYear"),
        DateGetMonth => Some("getMonth"),
        DateGetDate => Some("getDate"),
        DateGetDay => Some("getDay"),
        DateGetHours => Some("getHours"),
        DateGetMinutes => Some("getMinutes"),
        DateGetSeconds => Some("getSeconds"),
        DateGetMilliseconds => Some("getMilliseconds"),
        DateGetTimezoneOffset => Some("getTimezoneOffset"),
        _ => short_name_utc_or_set(b),
    }
}

const fn short_name_utc_or_set(b: Builtin) -> Option<&'static str> {
    use Builtin::*;
    match b {
        DateGetUTCFullYear => Some("getUTCFullYear"),
        DateGetUTCMonth => Some("getUTCMonth"),
        DateGetUTCDate => Some("getUTCDate"),
        DateGetUTCDay => Some("getUTCDay"),
        DateGetUTCHours => Some("getUTCHours"),
        DateGetUTCMinutes => Some("getUTCMinutes"),
        DateGetUTCSeconds => Some("getUTCSeconds"),
        DateGetUTCMilliseconds => Some("getUTCMilliseconds"),
        DateSetTime => Some("setTime"),
        DateSetFullYear => Some("setFullYear"),
        DateSetMonth => Some("setMonth"),
        DateSetDate => Some("setDate"),
        DateSetHours => Some("setHours"),
        DateSetMinutes => Some("setMinutes"),
        DateSetSeconds => Some("setSeconds"),
        DateSetMilliseconds => Some("setMilliseconds"),
        DateSetUTCHours => Some("setUTCHours"),
        DateSetUTCMinutes => Some("setUTCMinutes"),
        DateSetUTCSeconds => Some("setUTCSeconds"),
        DateSetUTCMilliseconds => Some("setUTCMilliseconds"),
        DateGetYear => Some("getYear"),
        DateSetYear => Some("setYear"),
        DateToLocaleString => Some("toLocaleString"),
        DateToLocaleDateString => Some("toLocaleDateString"),
        DateToLocaleTimeString => Some("toLocaleTimeString"),
        _ => None,
    }
}
