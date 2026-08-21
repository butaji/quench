use super::{Value, VmError};
use crate::ops::Builtin;
use std::sync::atomic::{AtomicU64, Ordering};

static SYMBOL_COUNTER: AtomicU64 = AtomicU64::new(0);
pub(crate) fn dispatch(builtin: Builtin, arguments: &[Value], _receiver: Option<&Value>) -> Option<Result<Value, VmError>> {
    if builtin == Builtin::Symbol { return Some(make_symbol(arguments)); }
    if let Some(name) = name(builtin) {
        let value = if builtin == Builtin::SymbolUnscopables { Value::String(format!("{name}\0")) } else { Value::String(name.to_string()) };
        return Some(Ok(value));
    }
    Some(match builtin { Builtin::SymbolFor => symbol_for(arguments), Builtin::SymbolKeyFor => symbol_key_for(arguments), _ => return None })
}
pub(crate) fn name(builtin: Builtin) -> Option<&'static str> {
    Some(match builtin {
        Builtin::SymbolIterator => "Symbol.iterator", Builtin::SymbolAsyncIterator => "Symbol.asyncIterator", Builtin::SymbolDispose => "Symbol.dispose", Builtin::SymbolAsyncDispose => "Symbol.asyncDispose", Builtin::SymbolUnscopables => "Symbol.unscopables", Builtin::SymbolToStringTag => "Symbol.toStringTag", Builtin::SymbolToPrimitive => "Symbol.toPrimitive", Builtin::SymbolHasInstance => "Symbol.hasInstance", Builtin::SymbolIsConcatSpreadable => "Symbol.isConcatSpreadable", Builtin::SymbolSpecies => "Symbol.species", Builtin::SymbolMatch => "Symbol.match", Builtin::SymbolReplace => "Symbol.replace", Builtin::SymbolSearch => "Symbol.search", Builtin::SymbolSplit => "Symbol.split", Builtin::SymbolMatchAll => "Symbol.matchAll", _ => return None,
    })
}
fn make_symbol(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::String(value)) = arguments.first() { if crate::conversion::is_symbol_string(value) { return Err(crate::value::error::throw_type_error("Symbol may not be used as a description")); } }
    let description = match arguments.first() { None | Some(Value::Undefined) => "\u{1}".to_string(), Some(value) => crate::conversion::to_string(value)? };
    let counter = SYMBOL_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(Value::String(format!("Symbol.{description}\0{counter}")))
}
pub(crate) fn legacy_symbol() -> Result<Value, VmError> { make_symbol(&[Value::String("IntlLegacyConstructedSymbol".to_string())]) }
fn symbol_for(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::String(value)) = arguments.first() { if crate::conversion::is_symbol_string(value) { return Err(crate::value::error::throw_type_error("Cannot convert a Symbol value to a string")); } }
    let key = crate::conversion::to_string(arguments.first().unwrap_or(&Value::Undefined))?;
    Ok(Value::String(format!("Symbol.for.{key}\0")))
}
fn symbol_key_for(arguments: &[Value]) -> Result<Value, VmError> {
    if arguments.first().is_some_and(|value| matches!(value, Value::Builtin(builtin) if name(*builtin).is_some())) { return Ok(Value::Undefined); }
    let Some(Value::String(value)) = arguments.first() else { return Err(crate::value::error::throw_type_error("Symbol.keyFor requires a symbol")); };
    let Some(value) = value.strip_prefix("Symbol.for.") else { return if crate::conversion::is_symbol_string(value) { Ok(Value::Undefined) } else { Err(crate::value::error::throw_type_error("Symbol.keyFor requires a symbol")) }; };
    Ok(Value::String(value.strip_suffix('\0').unwrap_or(value).to_string()))
}
