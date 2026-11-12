//! A function's BaseJIT body (`--b-jit`): the whole function stitched from stencils
//! (`base_region::function`) and published as a lane body, so the tier prologue enters it and
//! its leaves resume the interpreter as a Cranelift body's do (`lane_body_entry`,
//! `LaneExit`). A call site that recorded a stable callee calls its entry cell (the direct row,
//! the callee's descriptor owned by the method like a Cranelift body's `direct`), any other call
//! takes the helper row, and an instruction with no row leaves the body at its PC.

use super::*;
use super::base_region::callees::{Callees, InlineBody};
use super::base_region::function::{build_body, Body};
use super::base_region::Known;
use super::lane_facts::bind_callees;
use crate::code_dependencies::Invalidation;
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
    /// no shapes or slots, and its only Stable facts are the atoms its stores hold unnamed by
    /// method caches (`Body::atoms`) and the values its folded reads hold of prototypes
    /// (`Body::holders`), so it depends on nothing but its own entry guards, those, which retire it
    /// and set `invalidation` (the word its rows read, whose address its code names), and the
    /// callees it calls by their entries (`direct`, which it owns, and whose records it keeps).
    fn of_base_body(body: Body, function: u32, registers: u16, invalidation: Invalidation, (direct, closed): (Box<[DirectCallee]>, bool)) -> Option<Self> {
        let memory = super::code_heap::publishing("b-jit-body", function as usize, || CodeMemory::new(&body.code))?;
        // SAFETY: `base_region::function` links its code behind `entry_body`, of the lane body ABI.
        let entry = unsafe { std::mem::transmute::<*mut u8, BodyEntry>(memory.pointer()) };
        Some(Self {
            dependencies: super::lane_compile::LaneDependencies::of(std::iter::empty(), &direct), stable: super::lane_region::StableFacts { holders: body.holders, atoms: body.atoms, ..Default::default() }, invalidation, _code: memory, entry,
            recovery_space: Default::default(), fault_poll: direct.iter().any(|callee| callee.body.as_ref().is_some_and(|body| body.fault_poll)),
            names_shapes: !direct.is_empty(), inlined: Vec::new(), scratch_width: 0,
            caller_registers: registers, amortizes: body.loops || body.covered >= TIERS.lane_body.min_instructions || !direct.is_empty(),
            direct, closed, _records: body.records,
        })
    }
}

impl<H: Host> Vm<H> {
    /// `function`'s body for lane activations under `--b-jit`: what `parameters` (the entry's)
    /// showed and the places a predecessor left at (`generic`) specialize its rows, and `direct`
    /// (the callees each call site reached) its calls. Declined where the function has no row to
    /// run first, or is too short for its entry to pay (`amortizes_entry`).
    pub(in crate::vm) fn base_body_work(&mut self, program: &ResidualProgram, function: u32, generic: &[LaneExit], parameters: &[Value],
        caller: &super::super::dispatch_fast::LaneView, direct: &dyn Fn(&Self) -> Vec<DirectCandidates>) -> LaneBodyOutcome {
        let Some(body) = program.function_get(function as usize) else { return LaneBodyOutcome::Declined };
        let masks: Option<&std::rc::Rc<RegisterMasks>> = program.shared_register_root_masks(function);
        let code: Vec<_> = body.instructions().collect();
        if !super::base_region::covers(&code) { return LaneBodyOutcome::Declined }
        let Some(region) = LoopRegion::of_function(code.clone(), self.programs.constants(self.active_program), masks) else { return LaneBodyOutcome::Declined };
        let (ints, doubles) = observed(parameters);
        let callees = Callees::of(&code, &direct(self), &|callee| program.function_get(callee as usize).map(InlineBody::of));
        let helpers = super::process_lane_helpers::<H>();
        let invalidation = Invalidation::default();
        // Where the VM and the program run specialized, the code holds Stable facts: its stores run in
        // place for the atoms no method cache names, and its prototype-held reads fold to their values.
        let folds = self.specialized && program.specialized;
        let reads = |site: u16| self.native_field_read_descriptors(site);
        let known = Known {
            program, stores: folds.then(|| self.method_cache_atoms.bits()),
            reads: folds.then_some(&reads as &dyn Fn(u16) -> Vec<NativeFieldLoad>),
            invalidation: Some(invalidation.address()),
            constants: self.programs.constants(self.active_program),
        };
        // SAFETY: the callees' views and entry cells outlive the body that owns them (`DirectCallee`).
        let Ok(built) = (unsafe { build_body(&region, (function, body.registers), known, &widened(generic), [&ints, &doubles], &callees, helpers) }) else { return LaneBodyOutcome::Declined };
        let closed = callees.closed(&code);
        let Some(method) = LaneMethod::of_base_body(built, function, body.registers, invalidation, (callees.owned, closed)) else { return LaneBodyOutcome::Declined };
        if !method.amortizes_entry() { return LaneBodyOutcome::Short }
        bind_callees(&method.direct, caller);
        self.programs.publish_lane_method(method, &mut self.heap, self.method_cache_atoms.readers()).map_or(LaneBodyOutcome::Declined, LaneBodyOutcome::Published)
    }
}

#[cfg(test)]
#[path = "base_body/tests.rs"]
mod tests;
