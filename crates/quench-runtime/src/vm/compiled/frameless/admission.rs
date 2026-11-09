//! Admission of callees for frameless calls: a callee's function is admitted once per
//! collection epoch, and each closure of it takes the function's admission with its own
//! captures.
use super::{CallEntry, CACHE_ENTRIES, MATERIALIZATION_LIMIT, META_PARAMS, META_REGISTERS, THIS_NONE, THIS_RECEIVER};
use super::super::{Host, JsStepDispatch, ResidualProgram, Vm};
use crate::Value;

/// Admits `callee` for frameless calls from the payload's program and caches it: its
/// entry's address, or 0 where its calls keep frames. Admitted are plain user functions of
/// the running program (`CallPlan::in_loop`) whose whole state is their registers
/// (`CallPlan::registers_only`), whose body has a linked native entry, and whose
/// activations have not kept leaving.
///
/// # Safety
/// As `general_user_call`.
pub(in crate::vm) unsafe extern "C" fn general_frameless_admit<H: Host>(vm: u64, residual: u64, payload: u64, callee: u64) -> u64 {
    // SAFETY: as the caller promises; admission reads the heap and allocates nothing.
    let (vm, program, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram),
        &*(payload as *const JsStepDispatch)) };
    admit(vm, program, dispatch, Value::from_bits(callee)).map_or(0, |entry| entry as u64)
}

fn admit<H: Host>(vm: &mut Vm<H>, program: &ResidualProgram, dispatch: &JsStepDispatch, callee: Value) -> Option<*const CallEntry> {
    let entry = match vm.same_realm_user_target(callee).and_then(|target| admitted_function(vm, program, dispatch, target)) {
        Some(entry) => entry,
        None => admit_function(vm, program, dispatch, callee)?,
    };
    let entry = CallEntry { callee: callee.to_bits(), ..entry };
    let slot = (callee.to_bits() as usize) & (CACHE_ENTRIES - 1);
    vm.frameless.cache[slot] = entry;
    Some(&vm.frameless.cache[slot])
}

/// The admission of a fresh closure of a function admitted in this epoch: the function's,
/// with the closure's captures where its environment differs.
fn admitted_function<H: Host>(vm: &mut Vm<H>, program: &ResidualProgram, dispatch: &JsStepDispatch,
    (owner, function, env): (crate::vm::ProgramId, u32, Value)) -> Option<CallEntry> {
    let &(admitted, admitted_env, entry) = vm.frameless.functions.get(function as usize)?;
    if admitted != owner.raw() || owner != dispatch.program_id || entry.code == 0 || entry.epoch != vm.heap.collection_epoch() {
        return None;
    }
    Some(if env == admitted_env { entry } else { captures(vm, program, owner, env, entry) })
}

/// `entry` with the dense capture slots of a closure environment `env`, and whether they take
/// direct writes.
fn captures<H: Host>(vm: &mut Vm<H>, program: &ResidualProgram, owner: crate::vm::ProgramId, env: Value, entry: CallEntry) -> CallEntry {
    let capture = vm.target_capture(program, owner, env);
    // SAFETY: a live environment's dense slots stay in place through this collection epoch.
    let (values, len) = (!capture.is_null()).then(|| unsafe {
        crate::heap::EnvironmentSlots::native_storage_view(super::super::dispatch_fast::capture_reads(capture)) }).flatten()
        .unwrap_or((std::ptr::null(), 0));
    CallEntry { capture_values: values as u64, capture_len: len as u64,
        capture_stores: u64::from(super::super::dispatch_fast::capture_stores(capture).is_some()), ..entry }
}

/// Admits `callee`'s function for frameless calls (`general_frameless_admit`) and records it
/// for its later closures.
fn admit_function<H: Host>(vm: &mut Vm<H>, program: &ResidualProgram, dispatch: &JsStepDispatch, callee: Value) -> Option<CallEntry> {
    let (owner, function, env) = vm.in_loop_call_target(program, callee)?;
    if owner != dispatch.program_id { return None; }
    let body = program.function_get(function as usize)?;
    let plan = body.call_plan(program);
    if !plan.in_loop || !plan.registers_only || body.rest { return None; }
    let epoch = vm.heap.collection_epoch();
    if vm.frameless.materializations.get(&(owner.raw(), function))
        .is_some_and(|&(leaves, counted)| leaves >= MATERIALIZATION_LIMIT && counted == epoch) {
        return None;
    }
    let code = native_entry(vm, owner, function)?;
    let this = if body.is_arrow { THIS_NONE } else if body.strict { THIS_RECEIVER } else { vm.realm.globals.to_bits() };
    let entry = captures(vm, program, owner, env, CallEntry {
        epoch,
        code: code as u64,
        meta: u64::from(function) | (u64::from(body.registers) << META_REGISTERS) | (u64::from(body.params) << META_PARAMS),
        this,
        constructs: u64::from(Vm::<H>::inline_constructible(body)),
        ..CallEntry::default()
    });
    let index = function as usize;
    let functions = &mut vm.frameless.functions;
    if functions.len() <= index { functions.resize(index + 1, (0, Value::UNDEFINED, CallEntry::default())); }
    functions[index] = (owner.raw(), env, entry);
    Some(entry)
}

/// The native entry of `function` in `owner`'s linked code, which only native builds have.
#[cfg(feature = "native-execution")]
fn native_entry<H: Host>(vm: &Vm<H>, owner: crate::vm::ProgramId, function: u32) -> Option<usize> {
    vm.programs.native_js_code(owner)?.entries().get(function as usize).map(|code| *code as usize)
}

#[cfg(not(feature = "native-execution"))]
fn native_entry<H: Host>(_: &Vm<H>, _: crate::vm::ProgramId, _: u32) -> Option<usize> {
    None
}
