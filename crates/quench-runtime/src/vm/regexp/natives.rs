//! The RegExp family's natives: the constructor, `RegExp.prototype` methods and accessors,
//! `RegExp.escape`, the legacy statics and `@@species`. Each is a standalone `NativeFn` that the
//! native table reaches through `operations::native_entry`; the methods they run stay in
//! `regexp.rs` and `regexp/legacy.rs`.
use super::*;
use crate::vm::native_table::NativeFn;

/// The entry of each RegExp native, or `None` for a native the family does not own.
pub(in crate::vm) fn entry(native: Native) -> Option<NativeFn> {
    match native {
        Native::RegExp => Some(native_regexp_construct),
        Native::RegExpCompile => Some(native_regexp_compile),
        Native::RegExpExec => Some(native_regexp_exec),
        Native::RegExpTest => Some(native_regexp_test),
        Native::RegExpEscape => Some(native_regexp_escape),
        Native::RegExpSymbolMatch => Some(native_regexp_symbol_match),
        Native::RegExpSymbolSearch => Some(native_regexp_symbol_search),
        Native::RegExpSymbolReplace => Some(native_regexp_symbol_replace),
        Native::RegExpSymbolMatchAll => Some(native_regexp_symbol_match_all),
        Native::RegExpSymbolSplit => Some(native_regexp_symbol_split),
        Native::RegExpToString => Some(native_regexp_to_string),
        Native::RegExpSpecies => Some(native_regexp_species),
        Native::RegExpLegacyGetter => Some(native_regexp_legacy_getter),
        Native::RegExpLegacySetter => Some(native_regexp_legacy_setter),
        Native::RegExpGlobal
        | Native::RegExpIgnoreCase
        | Native::RegExpMultiline
        | Native::RegExpDotAll
        | Native::RegExpUnicode
        | Native::RegExpUnicodeSets
        | Native::RegExpSticky
        | Native::RegExpHasIndices => Some(native_regexp_flag),
        Native::RegExpSource => Some(native_regexp_source),
        Native::RegExpFlags => Some(native_regexp_flags),
        _ => None,
    }
}

fn native_regexp_construct(
    vm: &mut Vm,
    p: &ResidualProgram,
    _native: Native,
    _this: Value,
    args: &[Value],
) -> Result<Value, JsError> {
    vm.construct_regexp_native(p, args, None)
}

fn native_regexp_compile(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_compile_native(p, this, args)
}

fn native_regexp_exec(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_builtin_exec(p, this, args)
}

fn native_regexp_test(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_test(p, this, args)
}

fn native_regexp_escape(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_escape_native(p, args)
}

fn native_regexp_symbol_match(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_symbol_match(p, this, args)
}

fn native_regexp_symbol_search(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_symbol_search(p, this, args)
}

fn native_regexp_symbol_replace(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_symbol_replace(p, this, args)
}

fn native_regexp_symbol_match_all(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_symbol_match_all(p, this, args)
}

fn native_regexp_symbol_split(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_symbol_split(p, this, args)
}

fn native_regexp_to_string(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_to_string_native(p, this)
}

fn native_regexp_species(_vm: &mut Vm, _p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    Ok(this)
}

fn native_regexp_legacy_getter(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_legacy_getter_native(p, this)
}

fn native_regexp_legacy_setter(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_legacy_setter_native(p, this, args)
}

/// The eight flag accessors share one body, which reads the flag the native names.
fn native_regexp_flag(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_flag_native(p, native, this)
}

/// `RegExp.prototype.source`: the slot accessor reads the source the native names.
fn native_regexp_source(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_slot_native(p, native, this)
}

fn native_regexp_flags(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_flags_native(p, this)
}
