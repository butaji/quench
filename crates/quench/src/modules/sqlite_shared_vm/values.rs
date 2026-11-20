//! Values between JavaScript and the engine, binding of parameters, and the engine's errors
//! as `node:sqlite` throws them (`ERR_SQLITE_ERROR` with `errcode` and `errstr`).
use super::statement::Prepared;
use super::{throw, with_state, Failure};
use crate::modules::node_errors_shared_vm::{decorate_error, decorate_range_error};
use crate::modules::stream_shared_vm::{put, Context, Outcome};
use quench_runtime::{RootId, RootedError, ValueType};
use std::num::NonZero;
use turso_core::{LimboError, Numeric, Statement, Value};

/// `Number.MAX_SAFE_INTEGER`, the largest integer a number reads back exactly.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// SQLITE_RANGE: a bind position beyond the statement's parameters.
const SQLITE_RANGE: i32 = 25;

/// The bits of a result code that name its primary code.
const PRIMARY_CODE_MASK: i32 = 0xff;

/// The `ERR_SQLITE_ERROR` a failed engine call throws.
pub(crate) fn engine_error(c: &mut Context<'_>, error: &LimboError) -> RootedError {
    let code = error.sqlite_extended_result_code();
    sqlite_error(c, &error.to_string(), code)
}

pub(crate) fn sqlite_error(c: &mut Context<'_>, message: &str, code: i32) -> RootedError {
    match decorate_sqlite_error(c, message, code) {
        Ok(error) => c.throw(error),
        Err(failure) => failure,
    }
}

fn decorate_sqlite_error(c: &mut Context<'_>, message: &str, code: i32) -> Result<RootId, RootedError> {
    let error = c.error_rooted(message)?;
    decorate_error(c, error, "ERR_SQLITE_ERROR")?;
    let errcode = c.number(f64::from(code));
    put(c, error, "errcode", errcode)?;
    let errstr = c.string_rooted(errstr(code));
    put(c, error, "errstr", errstr)?;
    Ok(error)
}

/// `sqlite3_errstr` by primary result code.
const ERRSTR: [(i32, &str); 21] = [
    (1, "SQL logic error"),
    (2, "Internal logic error"),
    (3, "Access permission denied"),
    (4, "Query aborted"),
    (5, "database is locked"),
    (6, "database table is locked"),
    (7, "Out of memory"),
    (8, "attempt to write a readonly database"),
    (9, "Some kind of disk I/O error occurred"),
    (10, "Disk I/O error"),
    (11, "database disk image is malformed"),
    (13, "database or disk is full"),
    (14, "unable to open database file"),
    (17, "database schema has changed"),
    (18, "String or BLOB exceeded size limit"),
    (19, "constraint failed"),
    (20, "datatype mismatch"),
    (21, "bad parameter or other API misuse"),
    (23, "authorization denied"),
    (25, "column index out of range"),
    (26, "file is not a database"),
];

/// The `sqlite3_errstr` of `code`'s primary result code.
fn errstr(code: i32) -> &'static str {
    let primary = code & PRIMARY_CODE_MASK;
    ERRSTR.iter().find(|(known, _)| *known == primary).map_or("unknown error", |(_, text)| text)
}

/// A JavaScript value bound as a parameter: `label` names it in the error for a value that
/// cannot be bound. As in Node 22, only `null` binds as NULL; `undefined` is refused.
fn to_sql(c: &mut Context<'_>, value: RootId, label: &str) -> Result<Value, RootedError> {
    match c.value_type_rooted(value) {
        Some(ValueType::Null) => Ok(Value::Null),
        Some(ValueType::Number) => number_value(c, value),
        Some(ValueType::BigInt) => bigint_value(c, value),
        Some(ValueType::String) => {
            let text = c.string_text(value)?.unwrap_or_default();
            Ok(Value::from_text(text))
        }
        _ => match c.view_bytes_rooted(value) {
            Some(bytes) => Ok(Value::Blob(bytes)),
            None => {
                let message = format!("Provided value cannot be bound to SQLite parameter {label}.");
                Err(throw(c, Failure::Type("ERR_INVALID_ARG_TYPE", message)))
            }
        },
    }
}

/// An integral number binds as INTEGER, any other finite number as REAL.
fn number_value(c: &mut Context<'_>, value: RootId) -> Result<Value, RootedError> {
    let number = c.to_number(value)?;
    if number.fract() == 0.0 && number.abs() < i64::MAX as f64 {
        return Ok(Value::from_i64(number as i64));
    }
    Ok(Value::from_f64(number))
}

fn bigint_value(c: &mut Context<'_>, value: RootId) -> Result<Value, RootedError> {
    let text = c.to_string(value)?;
    match text.parse::<i64>() {
        Ok(integer) => Ok(Value::from_i64(integer)),
        Err(_) => Err(throw(c, Failure::Type("ERR_INVALID_ARG_VALUE", "BigInt value is too large to bind.".to_owned()))),
    }
}

/// The parameters an execution of statement `index` binds, each with its 1-based position: the
/// positional values, or the named object's own enumerable properties when the first argument
/// is a plain object.
pub(crate) fn collect(c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<Vec<(usize, Value)>, RootedError> {
    if let Some(&first) = args.first() {
        if is_named_object(c, first)? {
            return named(c, first, index);
        }
    }
    let mut bound = Vec::with_capacity(args.len());
    for (position, &arg) in args.iter().enumerate() {
        let value = to_sql(c, arg, &(position + 1).to_string())?;
        bound.push((position + 1, value));
    }
    Ok(bound)
}

fn is_named_object(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    Ok(c.is_object_rooted(value)? && c.view_bytes_rooted(value).is_none())
}

/// Each property in turn: its key must name a parameter, and then its value converts, labelled
/// by that parameter's position.
fn named(c: &mut Context<'_>, object: RootId, index: usize) -> Result<Vec<(usize, Value)>, RootedError> {
    let keys = c.enumerable_own_keys_rooted(object, false, true)?;
    let mut bound = Vec::with_capacity(keys.len());
    for key in keys {
        let name = c.string_text(key)?.unwrap_or_default();
        let Some(position) = with_state(c, |state| resolve(&state.statements[index], &name)) else {
            let message = format!("Unknown named parameter '{name}'");
            return Err(throw(c, Failure::Coded("ERR_INVALID_STATE", message)));
        };
        let value = c.get_property_rooted(object, key)?;
        let converted = to_sql(c, value, &position.to_string())?;
        bound.push((position, converted));
    }
    Ok(bound)
}

/// Binds `bindings` into `stmt` at their positions. A position beyond the statement's
/// parameters is the engine's SQLITE_RANGE, as Node reports it.
pub(crate) fn bind(stmt: &mut Statement, bindings: Vec<(usize, Value)>) -> Result<(), Failure> {
    let count = stmt.parameters_count();
    for (position, value) in bindings {
        if position > count {
            return Err(Failure::Sqlite(SQLITE_RANGE, "column index out of range".to_owned()));
        }
        stmt.bind_at(NonZero::new(position).expect("positions start at 1"), value)?;
    }
    Ok(())
}

/// The 1-based position of the parameter `name` names. A prefixed name is looked up as written;
/// a bare name also matches `:name`, `@name` and `$name` unless bare names are disallowed.
fn resolve(entry: &Prepared, name: &str) -> Option<usize> {
    let found = if is_prefixed(name) {
        entry.stmt.parameter_index(name)
    } else if entry.allow_bare {
        [':', '@', '$'].into_iter().find_map(|prefix| entry.stmt.parameter_index(&format!("{prefix}{name}")))
    } else {
        None
    };
    found.map(NonZero::get)
}

fn is_prefixed(name: &str) -> bool {
    matches!(name.as_bytes().first(), Some(b':' | b'@' | b'$'))
}

/// An engine value as JavaScript: `null`, a number, a bigint under `readBigInts`, a string,
/// or a `Uint8Array`.
pub(crate) fn sql_to_js(c: &mut Context<'_>, value: &Value, big: bool) -> Outcome {
    match value {
        Value::Null => Ok(c.null()),
        Value::Numeric(Numeric::Integer(integer)) => integer_value(c, *integer, big),
        Value::Numeric(Numeric::Float(float)) => Ok(c.number(f64::from(*float))),
        Value::Text(text) => Ok(c.string_rooted(text.value.as_ref())),
        Value::Blob(bytes) => c.uint8_array_from_rooted(bytes.to_vec()),
    }
}

fn integer_value(c: &mut Context<'_>, integer: i64, big: bool) -> Outcome {
    if big {
        return Ok(c.bigint_rooted(i128::from(integer)));
    }
    if integer.unsigned_abs() > MAX_SAFE_INTEGER {
        let message = format!("Value is too large to be represented as a JavaScript number: {integer}");
        return Err(throw(c, Failure::Range("ERR_OUT_OF_RANGE", message)));
    }
    Ok(c.number(integer as f64))
}

/// A row as an object with null prototype keyed by column name, or as an array.
pub(crate) fn row(c: &mut Context<'_>, names: &[String], values: &[Value], big: bool, arrays: bool) -> Outcome {
    let mut cells = Vec::with_capacity(values.len());
    for value in values {
        cells.push(sql_to_js(c, value, big)?);
    }
    if arrays {
        return c.array_rooted(&cells);
    }
    let object = c.null_object_rooted()?;
    for (name, cell) in names.iter().zip(cells) {
        put(c, object, name, cell)?;
    }
    Ok(object)
}

/// The `RangeError` helper the row values share, for callers outside this module.
pub(crate) fn range_error(c: &mut Context<'_>, code: &'static str, message: &str) -> RootedError {
    let built = c.range_error_rooted(message).and_then(|error| {
        decorate_range_error(c, error, code)?;
        Ok(error)
    });
    match built {
        Ok(error) => c.throw(error),
        Err(failure) => failure,
    }
}
