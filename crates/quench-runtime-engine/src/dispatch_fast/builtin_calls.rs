//! The native rows a call takes where the facts prove its callee is a builtin (`builtin_call_row`).
use super::*;
use quench_runtime::typed_ir::Identity;

/// The row of a `Call` record whose callee the facts prove is the builtin `Identity::Builtin(path)` that the realm binds to a
/// native (`Vm::native_for_path`): the row its native takes (`native_call_handler`), which the call's first miss would
/// otherwise choose at each miss (`script_call_resolve_miss`). The row tests the callee's identity, so a wrong guess only
/// leaves to `native_call_miss`. A native whose row reads the call's result or the length after it (`StringReplace`, an array
/// callback) keeps the miss's choice, which reads the run's state. `None` otherwise.
pub(super) fn builtin_call_row(vm: &quench_runtime::vm::Vm, facts: Option<&quench_runtime::compile::aot_facts::AotFacts>, function: u32, pc: u32,
    record: &LaneInstruction) -> Option<Handler> {
    let Identity::Builtin(path) = facts?.function_facts(function).cell_at(pc, record.b)?.identity else { return None };
    let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = vm.function_record(vm.native_for_path(path)?) else { return None };
    let native = *native;
    if matches!(native, Native::StringReplace) || native.is_array_callback() { return None; }
    Some(super::native_calls::native_call_handler(native, record, || false, || false))
}
