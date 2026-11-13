//! A function's BaseJIT body (`--b-jit`): the whole function stitched from stencils
//! (`base_region::function`) and published as a lane body, so the tier prologue enters it and
//! its leaves resume the interpreter as a Cranelift body's do (`lane_body_entry`,
//! `LaneExit`). A call site that recorded a stable callee calls its entry cell (the direct row,
//! the callee's descriptor owned by the method like a Cranelift body's `direct`), any other call
//! takes the helper row, and an instruction with no row leaves the body at its PC.

use super::*;
use super::base_region::callees::{Callees, InlineBody};
use super::base_region::function::{build_body, Body, BodySpec};
use super::base_region::{Known, RealmProof};
use super::lane_facts::bind_callees;
use crate::code_dependencies::{InstanceofFact, Invalidation};
use crate::vm::global_facts::GlobalTable;
use crate::bytecode::{Register, WideInstruction};
use super::lane_region::StableFacts;
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
    /// no shapes or slots, so it depends on nothing but its own entry guards, the callees it calls
    /// by their entries (`direct`, which it owns, and whose records it keeps) and its Stable facts
    /// (the globals that hold the callees it inlines, the atoms its stores hold unnamed by method
    /// caches and the values its folded reads hold of prototypes), whose change retires it and
    /// sets `invalidation`: the word its rows read, whose address its code names.
    fn of_base_body(body: Body, function: u32, registers: u16, direct: Box<[DirectCallee]>, closed: bool, invalidation: Invalidation) -> Option<Self> {
        // Each inlined callee's registers are the frame slots from the caller's end (`Frame::base`).
        let scratch_width = window_width(&body.frames, registers);
        let memory = super::code_heap::publishing("b-jit-body", function as usize, || CodeMemory::new(&body.code))?;
        // SAFETY: `base_region::function` links its code behind `entry_body`, of the lane body ABI.
        let entry = unsafe { std::mem::transmute::<*mut u8, BodyEntry>(memory.pointer()) };
        Some(Self {
            dependencies: super::lane_compile::LaneDependencies::of(std::iter::empty(), &direct), stable: StableFacts { globals: body.stable, holders: body.holders, atoms: body.atoms, instanceofs: body.instanceofs, layouts: body.layouts, slots: body.slot_types, ..Default::default() }, invalidation, _code: memory, entry,
            recovery_space: super::super::dispatch_fast::LaneRecoverySpace::inlined(&body.frames), fault_poll: direct.iter().any(|callee| callee.body.as_ref().is_some_and(|body| body.fault_poll)),
            names_shapes: !direct.is_empty(), scratch_width, framed: true,
            caller_registers: registers, amortizes: body.loops || body.covered >= TIERS.lane_body.min_instructions || !direct.is_empty(),
            direct, closed, _records: body.records, constructors: body.constructors, inlined: body.frames,
        })
    }
}

impl<H: Host> Vm<H> {
    /// The body of `function`, which a call site recorded `callee` running, where a body may inline it:
    /// its realm is the active one, so its rows run under the globals the caller's do (the fold is
    /// refused otherwise), and the inlining body proves that realm active at its entry (`RealmProof`).
    fn inlinable_callee(&self, program: &ResidualProgram, callee: Value, function: u32) -> Option<InlineBody> {
        self.same_realm_user_target(callee)?;
        InlineBody::of(program.function_get(function as usize)?, program.shared_register_root_masks(function))
    }

    /// The callees `candidates` (the callees each call site of `code`, the function's, reached, by
    /// PC) a stitch calls by their entries or inlines, and the Stable fact that holds each one's
    /// callee register (`Vm::stable_callees`).
    pub(in crate::vm::compiled) fn stitch_callees(&mut self, program: &ResidualProgram, function: u32, code: &[WideInstruction], candidates: &[DirectCandidates]) -> Callees {
        let stable = self.stable_callees(program, function, code, &Callees::recorded(code, candidates));
        let inline_body = |callee: &DirectCallee| self.inlinable_callee(program, callee.callee, callee.target.function);
        Callees::of(code, candidates, &inline_body, stable)
    }

    /// The realm a stitch finds its inlined callees in: the active one, which its entry proves active.
    pub(in crate::vm::compiled) fn realm_proof(&self) -> RealmProof {
        RealmProof { offset: u64::from(Self::realm_globals_offset().unsigned_abs()), global: self.realm.globals.to_bits() }
    }

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
        let callees = self.stitch_callees(program, function, &code, &direct(self));
        let helpers = super::process_lane_helpers::<H>();
        let invalidation = Invalidation::default();
        // Where the VM and the program run specialized, the code holds Stable facts: its stores run in
        // place for the atoms no method cache names, and its prototype-held reads fold to their values.
        let folds = self.specialized && program.specialized;
        let constructors = if folds { self.instanceof_constructors(program, &code, parameters) } else { Vec::new() };
        let presences = if folds { self.membership_facts(&code) } else { Vec::new() };
        // ...and its global names' accesses read the global object's slots under its layout.
        let globals = folds.then(|| self.global_table(program, function, &super::osr_compile::loop_unit(body), GlobalTable::Base));
        let reads = |site: u16| self.native_field_read_descriptors(site);
        let instanceofs = |pc: usize| constructors.get(pc).copied().flatten().and_then(|constructor| self.instanceof_fact(constructor).map(|fact| (constructor, fact)));
        let adds = |site: u16, atom, chained| self.native_field_add(site, atom, chained);
        let names = |site: u16, atom| self.native_function_name(site, atom);
        let known = Known {
            program, stores: folds.then(|| self.method_cache_atoms.bits()),
            reads: folds.then_some(&reads as &dyn Fn(u16) -> Vec<NativeFieldLoad>),
            adds: folds.then_some(&adds as &dyn Fn(u16, crate::bytecode::Atom, bool) -> Option<crate::staged::add_plan::AddSite>),
            names: folds.then_some(&names as &dyn Fn(u16, crate::bytecode::Atom) -> Option<(crate::staged::add_plan::Transition, u64)>),
            adds: folds.then_some(&adds as &dyn Fn(u16, crate::bytecode::Atom, bool) -> Option<crate::staged::add_plan::AddSite>),
            names: folds.then_some(&names as &dyn Fn(u16, crate::bytecode::Atom) -> Option<(crate::staged::add_plan::Transition, u64)>),
            invalidation: Some(invalidation.address()),
            constants: self.programs.constants(self.active_program),
            typeof_names: &self.typeof_names,
            instanceofs: folds.then_some(&instanceofs as &dyn Fn(usize) -> Option<(Value, InstanceofFact)>),
            presences: folds.then_some(presences.as_slice()),
            globals: globals.as_ref(),
        };
        let speculated = widened(generic);
        let spec = BodySpec { function, registers: body.registers, known, realm: self.realm_proof(), widened: &speculated, seen: [&ints, &doubles], kinds: &[] };
        // SAFETY: the callees' views and entry cells outlive the body that owns them (`DirectCallee`).
        let Ok(built) = (unsafe { build_body(&region, &spec, &callees, helpers) }) else { return LaneBodyOutcome::Declined };
        let closed = callees.closed(&code);
        let Some(method) = LaneMethod::of_base_body(built, function, body.registers, callees.owned, closed, invalidation) else { return LaneBodyOutcome::Declined };
        if !method.amortizes_entry() { return LaneBodyOutcome::Short }
        bind_callees(&method.direct, caller);
        self.programs.publish_lane_method(method, &mut self.heap, self.method_cache_atoms.readers()).map_or(LaneBodyOutcome::Declined, LaneBodyOutcome::Published)
    }
}

#[cfg(test)]
#[path = "base_body/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "base_body/forms.rs"]
mod forms;
