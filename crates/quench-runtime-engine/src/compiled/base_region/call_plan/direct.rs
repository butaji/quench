//! The direct row of a call and its record (`direct_part`), the places its leading arguments are passed from, and
//! the builtin call a region declines. Split from `call_plan.rs` for its size.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The pinned register holding argument `k` of the call, where the call passes it by place (the leading
/// `PLACED_ARGUMENTS` only) and the register is spilled: its place and its window register.
fn pinned_argument(plan: &CallPlan, k: usize) -> Option<(Place, Register)> {
    if k >= usize::from(plan.shape.argc).min(PLACED_ARGUMENTS) {
        return None;
    }
    let register = plan.shape.window + k as Register;
    let (place, _) = plan.spills.iter().copied().find(|&(_, spilled)| spilled == register)?;
    Some((place, register))
}

/// The place each leading argument is passed from (`CallRecord::argument_places`): its pinned register, else the window slot.
pub(super) fn argument_places(plan: &CallPlan) -> [u8; PLACED_ARGUMENTS] {
    std::array::from_fn(|k| pinned_argument(plan, k).map_or(Place::Mem.code(), |(place, _)| place.code()))
}

/// The direct row and the record it reads, whose address is its hole 0. `arguments` is the place each leading
/// argument is passed from (`argument_places`). The row compares the callee register unless a Stable fact holds
/// it or `unchecked` says the plan's closed arms leave no other callee.
pub(super) fn direct_part(shape: &CallShape, facts: &DirectFacts, arguments: [u8; PLACED_ARGUMENTS], proven_args: u64, unchecked: bool) -> Result<(CallPart, Box<CallRecord>), String> {
    // The callee's plain header: a native frame (NATIVE, as the u-jit's `push_native` writes it) of `argc`
    // arguments in the callee's function, which `native_frames::link_header` links by its descriptor.
    let packed = u64::from(facts.function) | PLAIN_FLAG | NATIVE_FLAG | ((shape.argc as u64) << ARGC_SHIFT);
    let record = Box::new(CallRecord {
        proven_args,
        callee: facts.callee,
        cell: facts.cell as u64,
        expected: facts.validity,
        call: facts.call,
        caller_base: facts.caller_base,
        packed,
        captures: facts.captures,
        global_this: facts.global_this.unwrap_or_default(),
        callee_register: shape.callee,
        this_register: shape.this,
        args_register: shape.window,
        result_register: shape.result,
        argument_places: arguments,
        descriptor: facts.descriptor,
        env: facts.env,
        capture: facts.capture,
        registers: u64::from(facts.registers),
        sloppy: u64::from(facts.global_this.is_some()),
    });
    let argc = u8::try_from(shape.argc).map_err(|e| e.to_string())?;
    let this = if facts.lexical { ThisRow::Lexical } else if facts.global_this.is_some() { ThisRow::Sloppy } else { ThisRow::Strict };
    let pinned = arguments.iter().any(|&place| place != Place::Mem.code());
    let stencil = call::stencil(CallRow::Direct { argc, this, checked: !facts.held && !unchecked, pinned })?;
    let mut holes = [0; HOLES];
    holes[DirectHole::Record as usize] = std::ptr::from_ref(&*record).addr() as u64;
    holes[DirectHole::CallPc as usize] = u64::from(shape.call_pc);
    Ok((part(stencil, holes, vec![Exit::Status]), record))
}

/// A call of the builtin's arity whose callee is that builtin declines the region: the interpreter runs the call in
/// place (`lane_call_in_place`), and with the call inside it the region measured slower than the interpreter
/// (TOD-2211). Any other call is planned as it is.
pub(super) fn decline_builtin(shape: &CallShape, builtin: Option<BuiltinCall>) -> Result<(), String> {
    let Some(BuiltinCall { row }) = builtin else { return Ok(()) };
    if shape.form != CallForm::Plain || shape.returning || shape.argc != row.arguments() {
        return Ok(());
    }
    Err(format!("the builtin call {row:?} runs in place in the interpreter"))
}
