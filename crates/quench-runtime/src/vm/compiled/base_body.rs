//! A function's BaseJIT body (`--b-jit`): the whole function stitched from stencils
//! (`base_region::function`) and published as a lane body, so the tier prologue enters it and
//! its leaves resume the interpreter as a Cranelift body's do (`lane_body_entry`,
//! `LaneExit`). It has no inlined frames and no direct callees: an instruction with no row
//! leaves the body at its PC.

use super::*;
use super::base_region::function::{build_body, Body};
use crate::bytecode::Register;
use crate::compile::liveness::RegisterMasks;

/// The registers an entry's parameters hold as int32s, and as other Numbers: the types the body's
/// entry rows assume (a parameter that is neither is a tagged word the rows test where they read it).
fn observed(parameters: &[Value]) -> (Vec<Register>, Vec<Register>) {
    let registers = |wanted: fn(Value) -> bool| (0..parameters.len()).filter(|k| wanted(parameters[*k])).map(|k| k as Register).collect();
    (registers(|value| value.as_int().is_some()), registers(|value| value.as_int().is_none() && value.as_number().is_some()))
}

/// The PCs a previous body left at: its rows there take the general (tagged) semantics.
fn widened(generic: &[LaneExit]) -> Vec<usize> {
    generic.iter().filter_map(|exit| match exit { LaneExit::At(pc) => Some(*pc), _ => None }).collect()
}

impl LaneMethod {
    /// The lane body `body`'s code makes of `function`, published to executable memory. It names
    /// no shapes, slots, callees or Stable facts, so it depends on nothing but its own entry guards.
    fn of_base_body(body: &Body, function: u32, registers: u16) -> Option<Self> {
        let memory = super::code_heap::publishing("b-jit-body", function as usize, || CodeMemory::new(&body.code))?;
        // SAFETY: `base_region::function` links its code behind `entry_body`, of the lane body ABI.
        let entry = unsafe { std::mem::transmute::<*mut u8, BodyEntry>(memory.pointer()) };
        Some(Self {
            dependencies: Default::default(), stable: Default::default(), invalidation: Default::default(), _code: memory, entry,
            recovery_space: Default::default(), fault_poll: false, names_shapes: false, inlined: Vec::new(), scratch_width: 0,
            caller_registers: registers, amortizes: body.loops || body.covered >= TIERS.lane_body.min_instructions,
            direct: Box::new([]), closed: false,
        })
    }
}

impl<H: Host> Vm<H> {
    /// `function`'s body for lane activations under `--b-jit`: what `parameters` (the entry's)
    /// showed and the places a predecessor left at (`generic`) specialize its rows. Declined where
    /// the function has no row to run first, or is too short for its entry to pay (`amortizes_entry`).
    pub(in crate::vm) fn base_body_work(&mut self, program: &ResidualProgram, function: u32, generic: &[LaneExit], parameters: &[Value]) -> LaneBodyOutcome {
        let Some(body) = program.function_get(function as usize) else { return LaneBodyOutcome::Declined };
        let masks: Option<&std::rc::Rc<RegisterMasks>> = program.shared_register_root_masks(function);
        let Some(region) = LoopRegion::of_function(body.instructions().collect(), self.programs.constants(self.active_program), masks) else { return LaneBodyOutcome::Declined };
        let (ints, doubles) = observed(parameters);
        let Ok(built) = build_body(&region, body.registers, &widened(generic), [&ints, &doubles]) else { return LaneBodyOutcome::Declined };
        let Some(method) = LaneMethod::of_base_body(&built, function, body.registers) else { return LaneBodyOutcome::Declined };
        if !method.amortizes_entry() { return LaneBodyOutcome::Short }
        self.programs.publish_lane_method(method, &mut self.heap, self.method_cache_atoms.readers()).map_or(LaneBodyOutcome::Declined, LaneBodyOutcome::Published)
    }
}

#[cfg(test)]
#[path = "base_body/tests.rs"]
mod tests;
