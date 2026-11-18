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
use crate::bytecode::{Class, Register, WideInstruction};
use super::lane_region::StableFacts;
use crate::compile::liveness::RegisterMasks;

/// The registers an entry's parameters hold as int32s, and as other Numbers: the types the body's
/// entry rows assume (a parameter that is neither is a tagged word the rows test where they read it).
fn observed(parameters: &[Value]) -> (Vec<Register>, Vec<Register>) {
    let registers = |wanted: fn(Value) -> bool| (0..parameters.len()).filter(|k| wanted(parameters[*k])).map(|k| k as Register).collect();
    (registers(|value| value.as_int().is_some()), registers(|value| value.as_int().is_none() && value.as_number().is_some()))
}

/// Whether `code` indexes a register an entry's parameter holds a typed array in: a body has no row of a typed
/// element (its loop regions do), so its first access of one would leave, and every entry after it.
fn indexes_typed_parameter(code: &[WideInstruction], is_typed: impl Fn(Register) -> bool) -> bool {
    code.iter().any(|instruction| match crate::vm::compiled::decoded(instruction) {
        Some(crate::staged::Operation::ElementRead { base, .. }) => is_typed(base),
        Some(crate::staged::Operation::ElementWrite { base, .. }) => is_typed(base),
        // Where the decode rejects an access (a result flag, or an operand that is no register), its
        // operands are read as the opcode reads them.
        None if instruction.op().class() == Class::GetIndex => instruction.operand_b().register_index().is_some_and(&is_typed),
        None if instruction.op().class() == Class::SetIndex => is_typed(instruction.register_b()),
        _ => false,
    })
}

/// The PCs a previous body left at: its rows there take the general (tagged) semantics.
fn widened(generic: &[LaneExit]) -> Vec<usize> {
    generic.iter().filter_map(|exit| match exit { LaneExit::At(pc) => Some(*pc), _ => None }).collect()
}

impl LaneMethod {
    /// The lane body `body`'s code makes of `function`, published to executable memory. It names
    /// no slots, and shapes only in its inline-cache rows, which a compaction retires
    /// (`names_shapes`), so otherwise it depends on nothing but its own entry guards, the callees it calls
    /// by their entries (`direct`, which it owns, and whose records it keeps) and its Stable facts
    /// (the globals that hold the callees it inlines, the atoms its stores hold unnamed by method
    /// caches and the values its folded reads hold of prototypes), whose change retires it and
    /// sets `invalidation`: the word its rows read, whose address its code names.
    fn of_base_body(tiers: &Tiers, body: Body, function: u32, registers: u16, direct: Box<[DirectCallee]>, (closed, realm_bound): (bool, bool), invalidation: Invalidation) -> Option<Self> {
        // Each inlined callee's registers are the frame slots from the caller's end (`Frame::base`).
        let scratch_width = window_width(&body.frames, registers);
        let memory = super::code_heap::publishing("b-jit-body", function as usize, || CodeMemory::new(&body.code))?;
        if let Some(directory) = native_dump_directory() {
            dump_native_code(directory, DumpSite::Body { function, scalar: None }, &memory, "");
        }
        // SAFETY: `base_region::function` links its code behind `entry_body`, of the lane body ABI.
        let entry = unsafe { std::mem::transmute::<*mut u8, BodyEntry>(memory.pointer()) };
        Some(Self {
            dependencies: super::lane_compile::LaneDependencies::of(std::iter::empty(), &direct), stable: StableFacts { globals: body.stable, holders: body.holders, atoms: body.atoms, instanceofs: body.instanceofs, layouts: body.layouts, slots: body.slot_types, ..Default::default() }, invalidation, _code: memory, entry,
            recovery_space: super::super::dispatch_fast::LaneRecoverySpace::inlined(&body.frames).reaching(body.scratch), fault_poll: direct.iter().any(|callee| callee.body.as_ref().is_some_and(|body| body.fault_poll)),
            names_shapes: body.names_shapes || !direct.is_empty(), exits: ExitTable::default(), scratch_width, framed: true,
            caller_registers: registers, frame_reach: body.scratch, amortizes: body.loops || body.covered >= tiers.amortizing_instructions(super::tier_policy::lane_body_tier()) || !direct.is_empty(),
            direct, closed, _records: body.records, constructors: body.constructors, inlined: body.frames, scalar: None, realm_bound,
        })
    }
}

/// The facts of a program's sites that its code folds (`Vm::site_facts`).
struct SiteFacts {
    constructors: Vec<Option<Value>>,
    presences: Vec<Option<crate::vm::in_presence::InPresence>>,
    globals: Option<crate::vm::global_facts::GlobalReads>,
}

impl<H: Host> Vm<H> {
    /// The body of `function`, which a call site recorded `callee` running, where a body may inline it:
    /// its realm is the active one, so its rows run under the globals the caller's do (the fold is
    /// refused otherwise), and the inlining body proves that realm active at its entry (`RealmProof`).
    fn inlinable_callee(&self, program: &ResidualProgram, callee: Value, function: u32) -> Option<InlineBody> {
        self.same_realm_user_target(callee)?;
        InlineBody::of(program.function_get(function as usize)?, program.shared_register_root_masks(function), self.programs.constants(self.active_program))
    }

    /// The callees `candidates` (the callees each call site of `code`, the function's, reached, by
    /// PC) a stitch calls by their entries or inlines, and the Stable fact that holds each one's
    /// callee register (`Vm::stable_callees`).
    pub(in crate::vm::compiled) fn stitch_callees(&mut self, program: &ResidualProgram, function: u32, code: &[WideInstruction], candidates: &[DirectCandidates]) -> Callees {
        let stable = self.stable_callees(program, function, code, &Callees::recorded(code, candidates));
        // An arrow inlines as any callee does: a `this` read is no class a site admits (`Stitch::classes`), and a leave
        // inside it makes its activation with its lexical `this` (`lane_call_this`).
        let inline_body = |callee: &DirectCallee| self.inlinable_callee(program, callee.callee, callee.target.function);
        Callees::of(code, candidates, &inline_body, stable)
    }

    /// The realm a stitch finds its inlined callees in: the active one, which its entry proves active.
    pub(in crate::vm::compiled) fn realm_proof(&self) -> RealmProof {
        RealmProof { offset: u64::from(Self::realm_globals_offset().unsigned_abs()), global: self.realm.globals.to_bits() }
    }

    /// The facts of the program's sites the code folds, where it `folds`: the constructors its
    /// `instanceof` rows name, its membership tests, and its global names' accesses, which read the
    /// global object's slots under its layout.
    fn site_facts(&mut self, folds: bool, program: &ResidualProgram, function: u32, (code, parameters): (&[WideInstruction], &[Value]), body: &crate::bytecode::Function) -> SiteFacts {
        if !folds { return SiteFacts { constructors: Vec::new(), presences: Vec::new(), globals: None }; }
        SiteFacts {
            constructors: self.instanceof_constructors(program, code, parameters),
            presences: self.membership_facts(code),
            globals: Some(self.global_table(program, function, &super::osr_compile::loop_unit(body), GlobalTable::Base)),
        }
    }

    /// Publishes the built body of `function` once its entry pays, binding the callees it names to `caller`.
    fn publish_base_body(&mut self, built: Body, (function, registers): (u32, u16), callees: Callees, (code, invalidation): (&[WideInstruction], Invalidation), caller: &super::super::dispatch_fast::LaneView) -> LaneBodyOutcome {
        let closed = callees.closed(code);
        // A body proves its realm at entry (`base_region::function::assemble`); its inlined callees are that
        // realm's (`inlinable_callee`), and so must its direct ones be for a realm switch to leave it standing.
        let realm_bound = callees.owned.iter().all(|callee| self.same_realm_user_target(callee.callee).is_some());
        let Some(method) = LaneMethod::of_base_body(self.programs.tiers(), built, function, registers, callees.owned, (closed, realm_bound), invalidation) else { return LaneBodyOutcome::Declined };
        // A body native callers call by its entry (`bind_callees`) pays for itself whatever its
        // length, as a Cranelift body does (`start_lane_method`).
        if !method.amortizes_entry() && !caller.called_by_bodies() { return LaneBodyOutcome::Short }
        bind_callees(&method.direct, caller);
        self.programs.publish_lane_method(method, &mut self.heap, self.method_cache_atoms.readers()).map_or(LaneBodyOutcome::Declined, LaneBodyOutcome::Published)
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
        if indexes_typed_parameter(&code, |register| parameters.get(usize::from(register)).is_some_and(|value| self.is_typed_array(*value))) { return LaneBodyOutcome::Declined }
        let Some(region) = LoopRegion::of_function(code.clone(), self.programs.constants(self.active_program), masks) else { return LaneBodyOutcome::Declined };
        let (ints, doubles) = observed(parameters);
        let callees = self.stitch_callees(program, function, &code, &direct(self));
        let invalidation = Invalidation::default();
        // Where the VM and the program run specialized, the code holds Stable facts: stores run in place for the atoms no method cache names, prototype-held reads fold.
        let folds = self.specialized && program.specialized;
        let SiteFacts { constructors, presences, globals } = self.site_facts(folds, program, function, (&code, parameters), body);
        let reads = |site: u16| self.native_field_read_descriptors(site);
        let instanceofs = |pc: usize| constructors.get(pc).copied().flatten().and_then(|constructor| self.instanceof_fact(constructor).map(|fact| (constructor, fact)));
        let adds = |site: u16, atom, chained| self.native_field_add(site, atom, chained);
        let names = |site: u16, atom| self.native_function_name(site, atom);
        let known = Known {
            program, stitch: &self.programs.tiers().stitch, stores: folds.then(|| self.method_cache_atoms.bits()),
            reads: folds.then_some(&reads as &dyn Fn(u16) -> Vec<NativeFieldLoad>),
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
        let Ok(built) = (unsafe { build_body(&region, &spec, &callees, super::process_lane_helpers::<H>()) }) else { return LaneBodyOutcome::Declined };
        self.publish_base_body(built, (function, body.registers), callees, (&code, invalidation), caller)
    }
}

#[cfg(test)]
#[path = "base_body/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "base_body/forms.rs"]
mod forms;
