//! The Intl layer's entry points in a build with the `intl` feature off. The core always names
//! them through `quench_runtime::vm::layer_hooks`, so this build defines each with the same
//! signature and refuses the Intl and Temporal behaviours with a `TypeError`; installing `Intl`
//! on a realm is a no-op, so a program that never names it sees no difference.

use quench_runtime::bytecode::ResidualProgram;
use quench_runtime::library_surface::Native;
use quench_runtime::vm::error::JsError;
use quench_runtime::vm::intl_core::Collation;
use quench_runtime::vm::layer_hooks::intl_signatures as signatures;
use quench_runtime::vm::Vm;
use quench_runtime::Value;
use std::rc::Rc;

fn unavailable(vm: &mut Vm, p: &ResidualProgram) -> JsError {
    vm.type_error(p, "Intl is unavailable in this build".into())
}

#[unsafe(no_mangle)]
pub fn quench_layer_intl_temporal_call(
    vm: &mut Vm,
    p: &ResidualProgram,
    _native: Native,
    _this: Value,
    _args: &[Value],
) -> Result<Value, JsError> {
    Err(unavailable(vm, p))
}
const _: signatures::intl_temporal_call = quench_layer_intl_temporal_call;

#[unsafe(no_mangle)]
pub fn quench_layer_intl_temporal_construct(
    vm: &mut Vm,
    p: &ResidualProgram,
    _native: Native,
    _args: &[Value],
    _new_target: Value,
) -> Result<Value, JsError> {
    Err(unavailable(vm, p))
}
const _: signatures::intl_temporal_construct = quench_layer_intl_temporal_construct;

#[unsafe(no_mangle)]
pub fn quench_layer_intl_format_primitive(
    vm: &mut Vm,
    p: &ResidualProgram,
    _value: Value,
    _args: &[Value],
) -> Result<Value, JsError> {
    Err(unavailable(vm, p))
}
const _: signatures::intl_format_primitive = quench_layer_intl_format_primitive;

/// No `Intl` global is installed, so a program that names it raises `ReferenceError` as it would
/// in a host without Intl; a program that never names it is unaffected.
#[unsafe(no_mangle)]
pub fn quench_layer_install_intl_for_realm(
    _vm: &mut Vm,
    _p: &ResidualProgram,
    _global: Value,
    _object_prototype: Value,
) -> Result<(), JsError> {
    Ok(())
}
const _: signatures::install_intl_for_realm = quench_layer_install_intl_for_realm;

#[unsafe(no_mangle)]
pub fn quench_layer_date_to_locale_string(
    vm: &mut Vm,
    p: &ResidualProgram,
    _native: Native,
    _this: Value,
    _args: &[Value],
) -> Result<Value, JsError> {
    Err(unavailable(vm, p))
}
const _: signatures::date_to_locale_string = quench_layer_date_to_locale_string;

#[unsafe(no_mangle)]
pub fn quench_layer_temporal_instant_constructor(vm: &mut Vm, p: &ResidualProgram) -> Result<Value, JsError> {
    Err(unavailable(vm, p))
}
const _: signatures::temporal_instant_constructor = quench_layer_temporal_instant_constructor;

#[unsafe(no_mangle)]
pub fn quench_layer_make_temporal_instant(
    vm: &mut Vm,
    p: &ResidualProgram,
    _epoch: i128,
    _new_target: Value,
) -> Result<Value, JsError> {
    Err(unavailable(vm, p))
}
const _: signatures::make_temporal_instant = quench_layer_make_temporal_instant;

#[unsafe(no_mangle)]
pub fn quench_layer_canonical_locale_list(
    vm: &mut Vm,
    p: &ResidualProgram,
    _locales: Option<Value>,
) -> Result<Vec<String>, JsError> {
    Err(unavailable(vm, p))
}
const _: signatures::canonical_locale_list = quench_layer_canonical_locale_list;

#[unsafe(no_mangle)]
pub fn quench_layer_locale_compare_collation(
    vm: &mut Vm,
    p: &ResidualProgram,
    _locales: Value,
    _options: Value,
) -> Result<Rc<Collation>, JsError> {
    Err(unavailable(vm, p))
}
const _: signatures::locale_compare_collation = quench_layer_locale_compare_collation;

#[unsafe(no_mangle)]
pub fn quench_layer_intl_segment_iterator_next(
    vm: &mut Vm,
    p: &ResidualProgram,
    _iterator: Value,
) -> Result<Value, JsError> {
    Err(unavailable(vm, p))
}
const _: signatures::intl_segment_iterator_next = quench_layer_intl_segment_iterator_next;
