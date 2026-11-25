//! A function's BaseJIT body (`--b-jit`): the whole function stitched from stencils
//! (`base_region::function`) and published as a lane body, so the tier prologue enters it and
//! its leaves resume the interpreter as a Cranelift body's do (`body_entry_in_lane`,
//! `LaneExit`). A call site that recorded a stable callee calls its entry cell (the direct row,
//! the callee's descriptor owned by the method like a Cranelift body's `direct`), any other call
//! takes the helper row, and an instruction with no row leaves the body at its PC.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::base_region::callees::{Callees, InlineBody};
use super::base_region::function::{build_body, Body, BodySpec, ClassProto};
use super::base_region::{Known, RealmProof, SymbolSite};
use super::lane_facts::bind_callees;
use super::admission::{refuse, Refusal};
use quench_runtime::code_dependencies::{InstanceofFact, Invalidation};
use quench_runtime::vm::global_facts::GlobalTable;
use quench_runtime::bytecode::{Atom, FieldLookup, Register, WideInstruction};
use super::lane_region::StableFacts;
use quench_runtime::compile::liveness::RegisterMasks;
use quench_runtime::typed_ir::Type;

/// The types an entry's parameters hold, by register: the types the body's entry rows assume (a parameter
/// that is neither an int32 nor a Number is `Type::immediate`'s own type, which no unboxed row accepts).
fn observed(parameters: &[Value]) -> Vec<Type> {
    parameters.iter().map(|value| Type::immediate(*value)).collect()
}

/// Whether `code` indexes a register an entry's parameter holds a typed array in: a body has no row of a typed
/// element (its loop regions do), so its first access of one would leave, and every entry after it.
fn indexes_typed_parameter(code: &[WideInstruction], is_typed: impl Fn(Register) -> bool) -> bool {
    // merge: kept HEAD: decoded_completed sees a result-flagged element access that `Operation::of` declines.
    code.iter().any(|instruction| match crate::compiled::decoded_completed(instruction) {
        Some(quench_runtime::staged::Operation::ElementRead { base, .. }) => is_typed(base),
        Some(quench_runtime::staged::Operation::ElementWrite { base, .. }) => is_typed(base),
        // A frame local's base (`ElementReadOperand`) is no register an entry parameter holds.
        _ => false,
    })
}

/// The PCs a previous body left at: its rows there take the general (tagged) semantics.
/// A loop's interpreter kernel, run on the loop's entry edge (`LaneHelper::KernelStep`, `base_region::kernel_entries`): the
/// record at `call_pc`, the loop's head, runs its kernel (`loop_kernel::enter_kernel`) over the running activation's
/// registers, as the interpreter's head does, and the body's rows continue the loop from where it left them. A body
/// running as a native callee runs it in its own frame, which the helper makes the context's
/// (`native_frames::in_innermost_frame`), as every helper of a native callee does. A head the kernel stood down from,
/// and a body running as a frameless leaf (whose records are its caller's), runs nothing here: the rows run the loop.
pub(super) unsafe extern "C" fn kernel_step_in_lane(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    let cx = context as usize as *mut quench_runtime::vm::engine_types::LaneContext;
    // SAFETY: the payload holds the running lane context, live while the body runs.
    if !unsafe { (*cx).leaf.call }.is_null() {
        return super::LANE_CALL_COMPLETED;
    }
    crate::dispatch_fast::native_frames::in_innermost_frame(cx, || {
        // SAFETY: the context's records and registers are the running activation's (`in_innermost_frame`), and its
        // view holds `call_pc`.
        let ip = unsafe { (*cx).base.add(call_pc as usize) };
        // SAFETY: the kernel runs over the frame's registers, which the chain spilled the pinned ones to.
        let ran = unsafe { crate::dispatch_fast::loop_kernel::kernel_at(ip) }.is_some_and(|kernel| unsafe { crate::dispatch_fast::loop_kernel::enter_kernel(kernel, cx, (*cx).registers, ip) });
        // A run wrote the registers the loop writes, whose values the body's pins and int proofs did not see: the
        // interpreter continues at the head from the frame as the kernel left it (a missed step's leave, which spills
        // nothing), where the loop's test now fails, as the interpreter's own head goes on after its kernel.
        if ran { super::LANE_CALL_MISSED } else { super::LANE_CALL_COMPLETED }
    })
}

/// Whether a loop of `code` has an interpreter kernel (`LoopKernel::recurrence`) that `cheaper`
/// says runs it in fewer instructions than the tier asking: its back jump lands before it, on a
/// header whose loop the kernel takes.
pub(super) fn kernel_loop(code: &[WideInstruction], constants: &[Value], function: &quench_runtime::bytecode::Function, live: Option<&RegisterMasks>,
    cheaper: impl Fn(&crate::dispatch_fast::loop_kernel::recur::Recurrence) -> bool) -> bool {
    code.iter().enumerate().any(|(backedge, jump)| {
        matches!(quench_runtime::staged::Operation::<(), ()>::decode(*jump, quench_runtime::staged::DecodeContext::EMPTY), Some(quench_runtime::staged::Operation::Jump))
            && (jump.jump_target() as usize) < backedge
            && crate::dispatch_fast::loop_kernel::LoopKernel::recurrence(code, constants, function, (jump.jump_target() as usize, backedge), live).is_some_and(|recurrence| cheaper(&recurrence))
    })
}

fn widened(generic: &[LaneExit]) -> Vec<usize> {
    generic.iter().filter_map(|exit| match exit { LaneExit::At(pc) => Some(*pc), _ => None }).collect()
}

#[quench_macros::ext(LaneMethodOps2)]
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
        // `entry_rows` follows `entry_body` (`base_region::rows_entry_offset`).
        let rows = super::base_region::row_entries(memory.pointer().addr(), body.number_guards, body.uses_this);
        Some(Self {
            dependencies: super::lane_compile::LaneDependencies::of(std::iter::empty(), &direct), stable: StableFacts { globals: body.stable, holders: body.holders, chains: body.chains, atoms: body.atoms, instanceofs: body.instanceofs, layouts: body.layouts, slots: body.slot_types, ..Default::default() }, invalidation, _code: Some(memory), entry,
            recovery_space: crate::dispatch_fast::LaneRecoverySpace::inlined(&body.frames).reaching(body.scratch), fault_poll: direct.iter().any(|callee| callee.body.as_ref().is_some_and(|body| body.fault_poll)),
            names_shapes: body.names_shapes || !direct.is_empty(), exits: ExitTable::default(), scratch_width, framed: true,
            caller_registers: registers, frame_reach: body.scratch, amortizes: body.loops || body.covered >= tiers.amortizing_instructions(crate::compiled::admission::Tier::Base) || !direct.is_empty(),
            direct, closed, _records: body.records, constructors: body.constructors, inlined: body.frames, scalar: None, rows, realm_bound, proven: None, summary: None, withdrawn: Default::default(),
        })
    }
}

#[quench_macros::ext(LaneMethodOps3)]
impl LaneMethod {
    /// The base body of an AOT image the system linker placed at `entry` (`aot_image::object`), of
    /// the lane body ABI as a b-jit body's (`base_region::function`), for a function of `registers`
    /// registers whose rows reach `scratch` past them. It inlines nothing and folds no Stable fact;
    /// its proven calls read the image's records (`BoundCalls`), and `invalidation` is the image's word.
    pub(crate) fn of_linked_base(entry: usize, registers: u16, scratch: usize, invalidation: Invalidation) -> Self {
        // SAFETY: the object linker places the base body behind `entry_body`, of the lane body ABI.
        let entry = unsafe { std::mem::transmute::<usize, BodyEntry>(entry) };
        Self {
            dependencies: super::lane_compile::LaneDependencies::of(std::iter::empty(), &[]), stable: StableFacts::default(), invalidation, _code: None, entry,
            recovery_space: crate::dispatch_fast::LaneRecoverySpace::inlined(&[]).reaching(scratch), fault_poll: false,
            names_shapes: false, exits: ExitTable::default(), scratch_width: 0, framed: true,
            caller_registers: registers, frame_reach: scratch, amortizes: true,
            direct: Box::new([]), closed: false, _records: Vec::new(), constructors: Box::default(), inlined: Vec::new(), scalar: None, rows: None, realm_bound: false, proven: None, summary: None, withdrawn: Default::default(),
        }
    }
}

/// The facts of a program's sites that its code folds (`Vm::site_facts`).
struct SiteFacts {
    constructors: Vec<Option<Value>>,
    presences: Vec<Option<quench_runtime::vm::in_presence::InPresence>>,
    globals: Option<quench_runtime::vm::global_facts::GlobalReads>,
}

#[quench_macros::ext(CompiledBaseBodyVm)]
impl Vm {
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
    pub(crate) fn stitch_callees(&mut self, program: &ResidualProgram, function: u32, code: &[WideInstruction], candidates: &[DirectCandidates]) -> Callees {
        let stable = self.stable_callees(program, function, code, &Callees::recorded(code, candidates));
        let natives = self.native_callees(program, function, code);
        // An arrow inlines as any callee does, unless it reads `this`: its lexical `this` is no receiver a site binds
        // (`inline::Refused::Receiver`), and a leave inside it makes its activation with its lexical `this` (`call_this_in_lane`).
        let inline_body = |callee: &DirectCallee| self.inlinable_callee(program, callee.callee, callee.target.function);
        Callees::of(code, candidates, &inline_body, stable, natives)
    }

    /// The realm a stitch finds its inlined callees in: the active one, which its entry proves active.
    pub(crate) fn realm_proof(&self) -> RealmProof {
        RealmProof { offset: u64::from(Self::realm_globals_offset().unsigned_abs()), global: self.realm.globals.to_bits() }
    }

    /// The facts of the program's sites the code folds, where it `folds`: the constructors its
    /// `instanceof` rows name, its membership tests, and its global names' accesses, which read the
    /// global object's slots under its layout.
    fn site_facts(&mut self, folds: bool, program: &ResidualProgram, function: u32, (code, parameters): (&[WideInstruction], &[Value]), body: &quench_runtime::bytecode::Function) -> SiteFacts {
        if !folds { return SiteFacts { constructors: Vec::new(), presences: Vec::new(), globals: None }; }
        SiteFacts {
            constructors: self.instanceof_constructors(program, code, parameters),
            presences: self.membership_facts(code),
            globals: Some(self.global_table(program, function, &super::osr_compile::loop_unit(body), GlobalTable::Base)),
        }
    }

    /// Publishes the built body of `function` once its entry pays, binding the callees it names to `caller`.
    fn publish_base_body(&mut self, built: Body, (function, registers): (u32, u16), callees: Callees, (code, invalidation): (&[WideInstruction], Invalidation), caller: &crate::dispatch_fast::LaneView) -> LaneBodyOutcome {
        let closed = callees.closed(code);
        // A body proves its realm at entry (`base_region::function::assemble`); its inlined callees are that
        // realm's (`inlinable_callee`), and so must its direct ones be for a realm switch to leave it standing.
        let realm_bound = callees.owned.iter().all(|callee| self.same_realm_user_target(callee.callee).is_some());
        let Some(mut method) = LaneMethod::of_base_body(self.programs.tiers(), built, function, registers, callees.owned, (closed, realm_bound), invalidation) else { return LaneBodyOutcome::Declined };
        method.exits = ExitTable::of_code(code);
        // A body native callers call by its entry (`bind_callees`) pays for itself whatever its
        // length, as a Cranelift body does (`start_lane_method`).
        if !method.amortizes_entry() && !caller.called_by_bodies() { refuse(self, Refusal::NotAmortized); return LaneBodyOutcome::Short }
        bind_callees(&method.direct, caller);
        { let (mut facts, programs) = self.fact_watch(); programs.publish_lane_method(method, &mut facts) }.map_or(LaneBodyOutcome::Declined, LaneBodyOutcome::Published)
    }

    /// `function`'s body for lane activations under `--b-jit`: what `parameters` (the entry's)
    /// showed and the places a predecessor left at (`generic`) specialize its rows, and `direct`
    /// (the callees each call site reached) its calls. Declined where the function has no row to
    /// run first, or is too short for its entry to pay (`amortizes_entry`).
    /// The unit `base_body_work` compiles `function` as, or the outcome the gate gives where it
    /// declines (`admission::specialize`): not covered, too short to repay its entry (refused as
    /// `NotAmortized`), or indexing a typed parameter.
    pub(crate) fn base_unit_or_outcome(&mut self, program: &ResidualProgram, function: u32, parameters: &[Value],
        caller: &crate::dispatch_fast::LaneView) -> Result<super::admission::CompiledUnit, LaneBodyOutcome> {
        // Native code reads slot representations, which the runtime keeps from its first compile on.
        self.keep_slot_representations();
        let Some(body) = program.function_get(function as usize) else { return Err(LaneBodyOutcome::Declined) };
        let code: Vec<_> = body.instructions().collect();
        let facts = super::admission::UnitFacts {
            covers: super::base_region::covers(&code),
            amortizes: super::body_may_amortize(self.programs.tiers(), body, crate::compiled::admission::Tier::Base),
            called_by_bodies: caller.called_by_bodies(),
            indexes_typed: indexes_typed_parameter(&code, |register| parameters.get(usize::from(register)).is_some_and(|value| self.is_typed_array(*value))),
            ..Default::default()
        };
        match super::admission::specialize(super::admission::UnitRequest { kind: super::admission::UnitKind::Base, facts }) {
            super::admission::Decision::Compile(unit) => Ok(unit),
            super::admission::Decision::Short => { refuse(self, Refusal::NotAmortized); Err(LaneBodyOutcome::Short) }
            super::admission::Decision::Declined => Err(LaneBodyOutcome::Declined),
            super::admission::Decision::Unpaid | super::admission::Decision::Base => unreachable!("a stencil body's gate decides only Short or Declined"),
        }
    }

    /// `base_body_work` for `function`, whose `unit` the gate cleared (`base_unit_or_outcome`).
    pub(crate) fn base_body_work(&mut self, unit: super::admission::CompiledUnit, program: &ResidualProgram, function: u32, generic: &[LaneExit], parameters: &[Value],
        caller: &crate::dispatch_fast::LaneView, direct: &dyn Fn(&Self) -> Vec<DirectCandidates>) -> LaneBodyOutcome {
        if !cfg!(feature = "jit") {
            return LaneBodyOutcome::Declined; // no run-time compile without the JIT: only image bodies run natively
        }
        let _ = unit;
        // What the body's rows leave to helpers is noted at this function's instructions (`quench_runtime::fallback`).
        let _focus = quench_runtime::fallback::focus(program, function, 0);
        let Some(body) = program.function_get(function as usize) else { return LaneBodyOutcome::Declined };
        let masks: Option<&std::rc::Rc<RegisterMasks>> = program.shared_register_root_masks(function);
        let code: Vec<_> = body.instructions().collect();
        // The region keeps the function's whole code (a function's span is all of it), which the rest reads back.
        let Some(region) = LoopRegion::of_function(code, self.programs.constants(self.active_program), masks) else { return LaneBodyOutcome::Declined };
        let seen = observed(parameters);
        let callees = self.stitch_callees(program, function, region.instructions(), &direct(self));
        let invalidation = Invalidation::default();
        // Where the VM and the program run specialized, the code holds Stable facts: stores run in place for the atoms no method cache names, prototype-held reads fold.
        let folds = self.specialized && program.specialized;
        let SiteFacts { constructors, presences, globals } = self.site_facts(folds, program, function, (region.instructions(), parameters), body);
        let classes = self.class_prototypes(program, function, region.instructions(), parameters);
        let reads = |site: u16| self.native_field_read_descriptors(site);
        let instanceofs = |pc: usize| constructors.get(pc).copied().flatten().and_then(|constructor| self.instanceof_fact(constructor).map(|fact| (constructor, fact)));
        let adds = |site: u16, atom, chained| self.native_field_add(site, atom, chained);
        let names = |site: u16, atom| self.native_function_name(site, atom);
        let fields = |site: u16| self.field_cache_filled(site);
        let known = Known {
            program, stitch: &self.programs.tiers().stitch, stores: folds.then(|| self.method_cache_atoms.bits()),
            reads: folds.then_some(&reads as &dyn Fn(u16) -> Vec<NativeFieldLoad>),
            adds: folds.then_some(&adds as &dyn Fn(u16, quench_runtime::bytecode::Atom, bool) -> Option<quench_runtime::staged::add_plan::AddSite>),
            names: folds.then_some(&names as &dyn Fn(u16, quench_runtime::bytecode::Atom) -> Option<(quench_runtime::staged::add_plan::Transition, u64)>),
            invalidation: Some(invalidation.address()),
            constants: self.programs.constants(self.active_program),
            typeof_names: &self.typeof_names,
            instanceofs: folds.then_some(&instanceofs as &dyn Fn(usize) -> Option<(Value, InstanceofFact)>),
            presences: folds.then_some(presences.as_slice()),
            fields: folds.then_some(&fields as &dyn Fn(u16) -> bool),
            globals: globals.as_ref(),
            classes: if folds { classes.as_slice() } else { &[] },
            shapes: &[],
        };
        let speculated = widened(generic);
        let spec = BodySpec { function, registers: body.registers, known, realm: Some(self.realm_proof()), widened: &speculated, seen: &seen, kinds: &[] };
        // SAFETY: the callees' views and entry cells outlive the body that owns them (`DirectCallee`).
        let Ok(built) = (unsafe { build_body(&region, &spec, &callees, super::process_lane_helpers()) }) else { return LaneBodyOutcome::Declined };
        self.publish_base_body(built, (function, body.registers), callees, (region.instructions(), invalidation), caller)
    }

    /// The prototypes `function`'s class-typed parameters enter with (`DeclaredTypes::parameter_classes`), each resolved
    /// from the value the entry observed in its register (`class_prototype_of`), with the named reads `code` makes of it.
    fn class_prototypes(&self, program: &ResidualProgram, function: u32, code: &[WideInstruction], parameters: &[Value]) -> Vec<ClassProto> {
        let (Some(declared), Some(body)) = (program.declared_types.as_deref(), program.function_get(function as usize)) else { return Vec::new() };
        let Some(start) = body.source_span.map(|span| span.start) else { return Vec::new() };
        declared.parameter_classes.iter().filter(|entry| entry.function == start)
            .filter_map(|entry| {
                let register = super::lane_region::scalar::parameter_register(body, usize::from(entry.parameter))?;
                let value = *parameters.get(usize::from(register))?;
                self.class_prototype_of(program, code, (register, value), entry.class)
            }).collect()
    }

    /// The class prototype the instance `value` in `register` enters with, where it is a direct object whose prototype's
    /// `constructor` is the class constructor (`is_class_constructor`) whose source span starts at `class`. Its members are
    /// the prototype's own data values of the names `code` reads of the register that the instance does not own (its exact
    /// shape holds no such name, else the prototype's value would not be the read's). Which prototype is a choice of what to
    /// fold: the entry checks the receiver's exact shape and link.
    fn class_prototype_of(&self, program: &ResidualProgram, code: &[WideInstruction], (register, instance): (Register, Value), class: u32) -> Option<ClassProto> {
        instance.direct_object_index()?;
        let receiver = self.object_data(instance)?;
        let shape = receiver.exact_shape();
        let link = receiver.proto_link();
        let prototype = Value::direct_object(link.object_record_id()?);
        let data = self.object_data(prototype)?;
        let slot = self.shape_slot(data.shape(), self.lookup_atom("constructor")?)?;
        let constructor = self.heap_reads().property_get(data, slot)?;
        let Some(quench_runtime::tier_surface::FunctionRecord { kind: quench_runtime::tier_surface::FunctionKind::User(_, index), .. }) = self.function_record(constructor) else { return None };
        let body = program.function_get(*index as usize)?;
        if !body.is_class_constructor || body.source_span.map(|span| span.start) != Some(class) { return None; }
        let members = code.iter().filter_map(|instruction| match instruction.field_lookup()? {
            FieldLookup::Atom { atom, base, .. } if base.register_index() == Some(register) => Some(atom),
            _ => None,
        }).filter(|atom| self.shape_slot(shape, *atom).is_none())
            .filter_map(|atom| Some((atom, self.own_data_bits(prototype, atom)?))).collect();
        Some(ClassProto { register, shape, link: link.bits(), holder: data.exact_shape(), members })
    }

    /// The bits of `object`'s own data property `atom`: `None` for an absent name, or where the shape may hold accessors
    /// (`shape_may_have_accessors`), whose slot is no data value.
    fn own_data_bits(&self, object: Value, atom: Atom) -> Option<u64> {
        let data = self.object_data(object)?;
        if self.shape_may_have_accessors(data.shape()) { return None; }
        let slot = self.shape_slot(data.shape(), atom)?;
        self.heap_reads().property_get(data, slot).map(|value| value.to_bits())
    }
}

/// A function's base body built for an AOT image (`aot_image::base`): no profile and no folded fact, its
/// calls direct only where the closed world proves their callee (`Callees::proven`), so the run-time
/// addresses it names are its helpers' and its call records', as symbols (`LaneHelpers::Ahead`).
pub(crate) struct ImageBody {
    pub(crate) code: Vec<u8>,
    pub(crate) symbols: Box<[SymbolSite]>,
    /// The code offset of each PC's first row, ascending by PC (`Body::entries`).
    pub(crate) entries: Box<[(u32, usize)]>,
    /// The PCs that take the generic row (`Body::generic`).
    pub(crate) generic: Box<[u32]>,
    /// Each direct call's site PC, proven callee and the record its row reads as planned (`Body::records`).
    pub(crate) calls: Box<[(u32, u32, quench_bjit::call_record::CallRecord)]>,
    /// The registers past the function's own its rows reach (`Body::scratch`), which a publication sizes its frame for.
    pub(crate) scratch: usize,
}

/// The base body of `function` for an AOT image, over the program and its closed-world `facts`.
pub(crate) fn image_body(program: &ResidualProgram, function: u32, facts: &quench_runtime::compile::aot_facts::AotFacts) -> Result<ImageBody, String> {
    let _focus = quench_runtime::fallback::focus(program, function, 0);
    let body = program.function_get(function as usize).ok_or("no such function")?;
    let code: Vec<WideInstruction> = body.instructions().collect();
    // A dense int `switch` lowers to a `WasmBranchTable`, which the base body plans as a compare tree
    // (`Step::Switch`); every other Wasm opcode is the Wasm tiers'.
    let planned = |instruction: &WideInstruction| quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::BranchTable)
        || super::base_region::covers(std::slice::from_ref(instruction));
    if let Some(wasm) = code.iter().find(|instruction| !planned(instruction)) {
        return Err(format!("function {function}: the wasm instruction {:?} has no base row", wasm.op()));
    }
    let callees = Callees::proven(program, function, &code, facts);
    let (length, live) = (code.len(), program.shared_register_root_masks(function));
    let reason = if length == 0 { "an empty function" } else { "no liveness masks" };
    let region = LoopRegion::of_whole_function(code, &[], live).ok_or_else(|| format!("function {function} ({length} instructions) has no base span: {reason}"))?;
    // An image names no heap value by its address: its constant loads take the immediates alone (`undefined`, `null`, a
    // boolean, a Number), and any other constant is the deleted marker, which no constant row takes (`value_row`).
    let constants: Vec<Value> = program.constants.iter().map(|constant| super::facts::immediate(constant).unwrap_or(Value::DELETED)).collect();
    let known = Known {
        program, stitch: &super::tier_policy::STATIC_ROWS.stitch, stores: None, reads: None, adds: None, names: None,
        invalidation: None, constants: &constants, typeof_names: &[], instanceofs: None, presences: None, fields: None, globals: None, classes: &[], shapes: &[],
    };
    let spec = BodySpec { function, registers: body.registers, known, realm: None, widened: &[], seen: &[], kinds: &[] };
    // SAFETY: a proven callee names no entry cell (its record is the image's, `Callees::proven`).
    let built = unsafe { build_body(&region, &spec, &callees, LaneHelpers::Ahead) }?;
    let calls = image_calls(&built, facts.function_facts(function))?;
    Ok(ImageBody { scratch: built.scratch, code: built.code, symbols: built.symbols, entries: built.entries, generic: built.generic, calls })
}

/// Each direct row's record with its site and callee: the rows' `Symbol::CallRecord`s in code order are
/// the records in row order (`Body::records`; an image body has no builtin row, whose record names none).
fn image_calls(built: &Body, facts: quench_runtime::compile::aot_facts::FunctionFacts<'_>) -> Result<Box<[(u32, u32, quench_bjit::call_record::CallRecord)]>, String> {
    let sites: Vec<u32> = built.symbols.iter().filter_map(|site| match site.symbol {
        crate::aot_image::Symbol::CallRecord { pc, .. } => Some(pc),
        _ => None,
    }).collect();
    if sites.len() != built.records.len() { return Err("a direct row names no call record".into()); }
    sites.into_iter().zip(&built.records).map(|(pc, record)| {
        let callee = facts.call_target(pc).ok_or("a direct row of an unproven call")?;
        Ok((pc, callee, **record))
    }).collect()
}

#[cfg(test)]
#[path = "base_body/tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "base_body/forms.rs"]
pub(crate) mod forms;
