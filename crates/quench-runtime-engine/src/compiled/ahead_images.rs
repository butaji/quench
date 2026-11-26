//! The ahead-of-time image of a program as a loadable file. `AheadImages` holds the program's
//! lowered `AotImage` (`aot_image::lower`), links it into the VM that loads it
//! (`aot_image::live::install`), and reads and writes the file (`aot_image::file`). A recording
//! bundles the images of one run into one file.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
#[cfg(feature = "jit")]
use std::collections::BTreeMap;
#[cfg(feature = "jit")]
use quench_runtime::vm::jit_backend::relocatable_code::RelocatableCode;
use crate::aot_image::{LaneImage, base::BaseImage, symbols::SymbolTable};
#[cfg(feature = "jit")]
use crate::aot_image::symbols::Symbol;
use quench_runtime::compile::aot_facts::AotFacts;
#[cfg(feature = "jit")]
use super::lane_unit::Origin;
use crate::aot_image::{self, AotImage, file::Words, live::{self, LiveImage}};
#[cfg(feature = "jit")]
#[path = "ahead_images/constructions.rs"]
pub(crate) mod constructions;
#[cfg(feature = "jit")]
#[path = "ahead_images/fields.rs"]
pub(crate) mod fields;

/// This build of the engine and the core (`build.rs`): the only build an ahead image runs on.
pub const AHEAD_BUILD_ID: u64 = match u64::from_str_radix(env!("QUENCH_ENGINE_BUILD_ID"), 16) { Ok(id) => id, Err(_) => 0 };

/// The link-time hook through which the core's `quench compile` reads this build (`native_link::NativeSupport::check_build`).
#[unsafe(no_mangle)]
fn quench_engine_build_id() -> u64 { AHEAD_BUILD_ID }
const _: fn() -> u64 = quench_engine_build_id;

pub use quench_runtime::vm::engine_types::AHEAD_BUILD_ID_PREFIX;

/// `AHEAD_BUILD_ID_PREFIX` and the build ID, kept in every object of this crate's library so
/// `quench compile` can read which build a runtime library is before it links an image against
/// it (`native_link::NativeSupport::check_build`): an image only runs on its own build.
#[used]
pub(crate) static AHEAD_BUILD_ID_MARK: [u8; 32] = {
    let id = env!("QUENCH_ENGINE_BUILD_ID").as_bytes();
    let mut mark = [0u8; 32];
    let mut at = 0;
    while at < 16 {
        mark[at] = AHEAD_BUILD_ID_PREFIX[at];
        mark[16 + at] = id[at];
        at += 1;
    }
    mark
};

/// What names a program: a hash of its source text and every function's registers and opcodes
/// (atom numbers are per VM, so no operand), which an image is valid for and no other (plus the helper count the code was linked against; it is a validity key, never a reason to treat a program specially).
pub fn ahead_program_fingerprint(program: &ResidualProgram) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mix = |hash: u64, bytes: &[u8]| bytes.iter().fold(hash, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME));
    let mut hash = mix(FNV_OFFSET, &(super::LaneHelper::COUNT as u32).to_le_bytes());
    hash = mix(hash, program.source_text().unwrap_or_default().as_bytes());
    // Each function as the image lowered it (`lowered_function`): a tier-0 record and its promoted
    // record differ in opcodes, so a program fingerprinted before and after a promotion would not match.
    for index in 0..program.function_count() {
        let function = program.lowered_function(index);
        hash = mix(hash, &function.registers.to_le_bytes());
        hash = function.instructions().fold(hash, |hash, instruction| mix(hash, &(instruction.op() as u16).to_le_bytes()));
    }
    hash
}

/// The calls the unit of an image's lane body inlines: each call whose target the closed world
/// proves (`AotFacts::call_target`), bound by its function, which its admission does not compare
/// (`Lowering::closed_call`), to a callee whose leaves restart the call (`numeric_restartable`).
fn closed_targets(program: &ResidualProgram, facts: &AotFacts, (caller, pc): (u32, u32)) -> Vec<lane_unit::CallTarget> {
    let target = facts.function_facts(caller).call_target(pc).filter(|&callee| super::lane_facts::numeric_restartable(program, callee));
    target.map(|function| lane_unit::CallTarget { callee: lane_unit::CalleeBinding::Function, function }).into_iter().collect()
}

/// The unit of `function`'s lane body under the closed world's call targets (`lane_unit::unit`, its
/// default budget); `None` where a frame it inlines would leave by making the callee's activation.
fn unit_of(program: &ResidualProgram, function: u32, facts: &AotFacts) -> Option<lane_unit::Unit> {
    unit_or_refusal(program, function, facts).ok()
}

/// `unit_of`, or why there is none.
fn unit_or_refusal(program: &ResidualProgram, function: u32, facts: &AotFacts) -> Result<lane_unit::Unit, &'static str> {
    let body = program.function_get(function as usize).ok_or("no such function")?;
    let unit = lane_unit::unit(program, function, body, &|caller, pc| closed_targets(program, facts, (caller, pc)), &|callee| program.function_get(callee as usize))
        .ok_or("no lane unit")?;
    if !unit.frames.iter().all(|frame| frame.recovery == lane_unit::CallRecovery::RestartCall) { return Err("an inlined frame does not restart"); }
    Ok(unit)
}

/// The code of the lane body `unit` lowers to under `facts` (`LaneHelpers::Ahead`), and whether
/// it polls for faults; `None` where it gives no native record.
#[cfg(feature = "jit")]
fn lowered_code(program: &ResidualProgram, unit: &lane_unit::Unit, function: u32, (facts, global_reads): (VmFacts<'_>, &quench_runtime::vm::global_facts::GlobalReads),
    fields: &lane_region::FieldSites, target: &quench_runtime::vm::jit_backend::NativeTarget) -> Result<(RelocatableCode, bool, ExitTable), &'static str> {
    // An image body is the lane projection's first body: it lowers from the leave set a JIT body starts
    // with (none), so every fact holds where it is proved, and a miss leaves as the JIT body's does.
    let admitted = super::admission::ahead_unit(function).ok_or("the image lowering is declined")?;
    let mut lowered = lane_region::lower_body(program, function, unit, LaneHelpers::Ahead, fields,
        global_reads, facts, &[], &BTreeMap::new(), &[], None, &[], Default::default(), admitted).ok_or("the lane lowering declines")?;
    if lowered.region.native == 0 { return Err("no native record"); }
    let native = std::mem::replace(&mut lowered.region.function, ir::Function::new());
    let isa = quench_runtime::vm::jit_backend::ahead_lane_isa(target).ok_or("no lane ISA")?;
    let code = quench_runtime::vm::jit_backend::compile_function(native, &*isa, Some(admitted.code_ceiling()), |compiled, body| {
        RelocatableCode::extract_or_reason(compiled, body).map_err(|why| {
            if quench_runtime::trace::Channel::AotLanes.on() { eprintln!("aot-lanes: function {function}: {why}"); }
            NativeCompileRejection::Relocations
        })
    }).map_err(|rejection| rejection_reason(function, rejection))?;
    Ok((code, lowered.region.fault_poll, std::mem::take(&mut lowered.region.exits)))
}

/// The message a rejected image body is refused with, printing the structured rejection under
/// `QUENCH_TRACE=aot-lanes` (Cranelift's own error is printed where it is raised).
#[cfg(feature = "jit")]
fn rejection_reason(function: u32, rejection: NativeCompileRejection) -> &'static str {
    if quench_runtime::trace::Channel::AotLanes.on() { eprintln!("aot-lanes: function {function}: image body rejected: {rejection:?}"); }
    match rejection {
        NativeCompileRejection::CodeSizeBudget => "code past the lane body budget",
        NativeCompileRejection::Relocations => "a relocation the image cannot name",
        _ => "Cranelift rejects the code",
    }
}

/// The lane body of `function` for an AOT image under the closed-world `facts`, whose leaves
/// return their exit words as a JIT lane's do (`AheadLeaves`). The whole-program values of `frozen` fold at their reads, each read checked where it is made
/// (`GlobalRead::Frozen`, `VmFacts::frozen_fields`), as the JIT's frozen division folds them. A
/// construction the closed world proves keeps its receiver as its fields (`constructions`). A call
/// the closed world proves reads its callee's words from the base body's record (`proven_calls`).
/// `None` where it gives no native record.
#[cfg(feature = "jit")]
pub(crate) fn closed_lane_image(program: &ResidualProgram, function: u32, (facts, frozen): (&AotFacts, &ClosedFrozen),
    (_base, calls): (&BaseImage, &[aot_image::CallSite]), symbols: &mut SymbolTable, target: &aot_image::ImageTarget,
) -> Result<LaneImage, &'static str> {
    let unit = unit_or_refusal(program, function, facts)?;
    let proven = proven_calls(program, &unit, function, calls, symbols);
    let plans = constructions::construct_plans(program, &unit, function, facts, symbols);
    let strings = string_constants(program, &unit, symbols);
    let fields = fields::guessed_fields(program, &unit, facts, symbols);
    let plan_of = |caller: u32, pc: u32| plans.get(&(caller, pc)).cloned();
    let global_reads = super::frozen::frozen_unit_reads(program, &unit, function, &frozen.globals);
    let facts = VmFacts { ahead: Some(lane_region::AheadLeaves { calls: &proven, strings: &strings }), closed: Some(facts),
        frozen_fields: Some(&frozen.fields), construct_plans: Some(&plan_of), ..VmFacts::STATIC };
    let crosses = crosses_bodies(&unit, &proven);
    let (code, fault_poll, exits) = lowered_code(program, &unit, function, (facts, &global_reads), &fields, target)?;
    // A lane body's call through the VM's call helper steps its callee's records, where the base
    // body binds and enters the callee's body: richards' IdleTask.run (`this.scheduler.release(packet)`)
    // cost 5x the whole run as a lane.
    // A member of a global name (`Math.sqrt`) is left out: the helper runs a native in place.
    if calls_through_helper(&code) && calls_script(&unit, &proven) { return Err("calls through the VM's helper"); }
    // A lane body that polls and makes a call the closed world does not prove enters its callee's body
    // through the VM's call helper, where the base body binds its callee directly: deltablue's drivers
    // cost 3.4x the base body as lanes (4.46G -> 15.3G Ir). A proven call reads its callee from the
    // base body's record (`proven_calls`), so a loop whose every call is proven runs as a lane.
    if fault_poll && crosses { return Err("polls and calls through the VM's helper"); }
    Ok(LaneImage { code, fault_poll, exits })
}

#[cfg(not(feature = "jit"))]
pub(crate) fn closed_lane_image(_: &ResidualProgram, _: u32, _: (&AotFacts, &ClosedFrozen),
    _: (&BaseImage, &[aot_image::CallSite]), _: &mut SymbolTable, _: &aot_image::ImageTarget,
) -> Result<LaneImage, &'static str> {
    Err("ahead image lowering requires the jit feature")
}

/// The slot observations the guessed field reads of `code` depend on (`fields`): each guard its
/// loader resolved to a shape whose slot holds a type within Number, which a widening may recode to
/// raw doubles, where float slots are on; under tagged slots no word is raw.
fn guard_dependencies(image: &AotImage, vm: &mut Vm, code: &quench_runtime::vm::jit_backend::relocatable_code::RelocatableCode) -> Box<[quench_runtime::code_dependencies::SlotDependency]> {
    if !quench_runtime::vm::switches::float_slots() { return Box::new([]); }
    let mut keys: Vec<u32> = code.relocations.iter().filter(|relocation| relocation.namespace == super::lane_region::relocs::RelocKind::GuardShape as u32)
        .map(|relocation| relocation.key).collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter().filter_map(|key| aot_image::symbols::guarded_shape(image, vm, key))
        .filter(|(_, _, ty)| *ty != quench_runtime::typed_ir::Type::NEVER && ty.is_subtype(quench_runtime::typed_ir::Type::NUMBER))
        .map(|(shape, slot, ty)| quench_runtime::code_dependencies::SlotDependency { shape, slot, ty }).collect()
}

/// Whether `unit` may enter another body through the VM's call: a call of any form or a construction
/// that it neither inlines (a call whose frame the unit holds runs in the body; its alternatives' last
/// leaves at the call, as any guard does) nor calls through a proven record (`proven`).
#[cfg(feature = "jit")]
fn crosses_bodies(unit: &lane_unit::Unit, proven: &BTreeMap<usize, lane_region::ProvenCall>) -> bool {
    use quench_runtime::staged::meaning::{means, Meaning};
    let inlined = |unit_pc: usize| {
        let site = match unit.origins.get(unit_pc) {
            Some(Origin::Caller(pc)) => (None, *pc),
            Some(Origin::Inlined { frame, pc }) => (Some(*frame), *pc),
            None => return false,
        };
        unit.frames.iter().any(|frame| (frame.parent, frame.call) == site)
    };
    unit.instructions.iter().enumerate().any(|(unit_pc, instruction)| !inlined(unit_pc) && !proven.contains_key(&unit_pc)
        && [Meaning::Call, Meaning::MethodCall, Meaning::Construct].into_iter().any(|meaning| means(*instruction, meaning)))
}

/// Whether `unit` makes a call `proven` does not name whose callee may be a script function: any
/// but a member read of a global name (`Math.sqrt(x)`), which is a native wherever the program
/// writes no such member (the base body would call it through the same helper).
#[cfg(feature = "jit")]
fn calls_script(unit: &lane_unit::Unit, proven: &BTreeMap<usize, lane_region::ProvenCall>) -> bool {
    use quench_runtime::staged::meaning::{means, Meaning};
    let defined = |register, before: usize| (0..before).rev().find(|at| unit.instructions[*at].written_registers().any(|written| written == register));
    let global_member = |pc: usize, callee| defined(callee, pc).is_some_and(|read| {
        let field = unit.instructions[read];
        let base = super::meaning_compiled::is_get_field(field).then(|| field.field_lookup()).flatten();
        let Some(quench_runtime::bytecode::FieldLookup::Atom { base, .. }) = base else { return false };
        base.register_index().and_then(|object| defined(object, read)).is_some_and(|load| unit.instructions[load].op().loads_name())
    });
    unit.instructions.iter().enumerate().any(|(pc, instruction)| (means(*instruction, Meaning::Call) || means(*instruction, Meaning::MethodCall))
        && !proven.contains_key(&pc) && !global_member(pc, instruction.register_b()))
}

/// Whether `code` calls the VM's call helper (`LaneHelper::Call`), the lowering's own record of a
/// call it neither proved nor made in place.
#[cfg(feature = "jit")]
fn calls_through_helper(code: &RelocatableCode) -> bool {
    code.relocations.iter().any(|relocation| relocation.namespace == lane_region::relocs::RelocKind::Helper as u32
        && relocation.key == LaneHelper::Call as u32)
}

/// By constant pool slot, the heap value key of each string constant `unit` loads (`HeapValueRecipe::String`).
#[cfg(feature = "jit")]
fn string_constants(program: &ResidualProgram, unit: &lane_unit::Unit, symbols: &mut SymbolTable) -> BTreeMap<usize, u32> {
    unit.instructions.iter()
        .filter(|instruction| quench_runtime::staged::meaning::means(*instruction, quench_runtime::staged::meaning::Meaning::LoadConst))
        .filter_map(|instruction| {
            let slot = instruction.constant_index();
            let quench_runtime::bytecode::Constant::String(text) = program.constants.get(slot)? else { return None };
            let atom = symbols.text_atom(text);
            let Symbol::HeapValue(key) = symbols.heap_value(aot_image::HeapValueRecipe::String(atom)) else { return None };
            Some((slot, key))
        }).collect()
}

/// By unit PC, the caller calls the closed world proves that the base body planned a record for
/// (`calls`, its `AotImage::calls`): the lane body calls each through the same record, which the
/// loader binds (`BoundCalls::bind`) and the lane reads by `Symbol::CallRecord`. An arrow is left
/// to the VM's call, as the JIT's lane leaves it (`direct_callee_in_lane`): it binds no receiver.
#[cfg(feature = "jit")]
fn proven_calls(program: &ResidualProgram, unit: &lane_unit::Unit, function: u32, calls: &[aot_image::CallSite], symbols: &mut SymbolTable)
    -> BTreeMap<usize, lane_region::ProvenCall> {
    unit.origins.iter().enumerate().filter_map(|(pc, origin)| {
        let Origin::Caller(record) = *origin else { return None };
        let site = calls.iter().find(|site| site.function == function && site.pc == record)?;
        let callee = program.function_get(site.callee as usize)?;
        let this = super::base_region::callees::ThisForm::of(callee);
        if this.lexical { return None; }
        let record = symbols.intern(Symbol::CallRecord { function: site.function, pc: site.pc });
        Some((pc, lane_region::ProvenCall { function: site.callee, registers: callee.registers, sloppy: this.sloppy, record }))
    }).collect()
}

pub use quench_runtime::vm::engine_types::AheadImages;


use quench_runtime::vm::engine_types::recorded;

#[quench_macros::ext(AheadImagesOps1)]
impl AheadImages {
    /// How many functions the image has.
    pub fn len(&self) -> usize { self.image.functions.len() }

    /// The file of this image (`from_bytes` reads it back).
    pub fn to_bytes(&self) -> Vec<u8> { aot_image::file::to_bytes(&self.image) }

    /// The image the system linker placed in this executable (`aot_image::object`), as its entry
    /// table lists it: its file read back, its code and records used where they are placed.
    pub(crate) fn of_linked(table: &live::LinkedTable) -> Result<Self, String> {
        let image = aot_image::file::from_bytes(table.file)?;
        let linked = live::linked(&image, table)?;
        Ok(Self { linked: Some(linked), placed: Some(table.entries.as_ptr().addr()), ..Self::of(image) })
    }

    /// The image `to_bytes` wrote; an error names what the file lacks.
    pub fn from_bytes(file: &[u8]) -> Result<Self, String> { Ok(Self::of(aot_image::file::from_bytes(file)?)) }

    /// Starts collecting the images of every program this thread stores (`finish_recording`).
    #[cfg(feature = "jit")]
    pub fn start_recording() {
        *recorded() = Some(quench_runtime::vm::engine_types::Recording {
            lower: |program| AheadImages::compile(program),
            lower_wasm: |vm, id, program, bindings| AheadImages::compile_wasm(vm, id, program, bindings),
            images: Vec::new(),
        });
    }

    /// An execution-only build records no images (it carries no lowering).
    #[cfg(not(feature = "jit"))]
    pub fn start_recording() {}

    /// What was recorded since `start_recording`, which stops.
    pub fn finish_recording() -> Vec<AheadImages> { recorded().take().map(|recording| recording.images).unwrap_or_default().into_iter().map(Self::of).collect() }

    /// A file of several programs' images: each as `to_bytes` wrote it, after its length.
    pub fn bundle(all: &[AheadImages]) -> Vec<u8> {
        let mut out = (all.len() as u32).to_le_bytes().to_vec();
        for images in all {
            let bytes = images.to_bytes();
            out.extend((bytes.len() as u32).to_le_bytes());
            out.extend(bytes);
        }
        out
    }

    /// The images `bundle` wrote.
    pub fn unbundle(file: &[u8]) -> Result<Vec<AheadImages>, String> {
        let mut words = Words(file);
        (0..words.half()?).map(|_| { let length = words.half()? as usize; Self::from_bytes(words.bytes(length)?) }).collect()
    }
}

/// Whether `direct`, a closure of function `callee`, is what a proven site's row was planned for: it
/// binds `this` as the row does (`ThisForm`), and its cell holds the facts of a plain activation of
/// its window (as `DirectFacts::describes` admits a JIT plan's).
fn admits_proven(program: &ResidualProgram, callee: u32, direct: &DirectCallee) -> bool {
    let Some(body) = program.function_get(callee as usize) else { return false };
    let this = super::base_region::callees::ThisForm::of(body);
    direct.sloppy.is_some() == this.sloppy && direct.lexical == this.lexical && plain_cell(direct, body.registers)
}

/// Whether `callee`'s entry cell holds the facts of a plain activation of `registers` registers, the
/// facts an image's direct row is planned with (`callees::proven_facts`).
fn plain_cell(callee: &DirectCallee, registers: u16) -> bool {
    // SAFETY: a direct callee's cell is its function's live entry cell (`DirectCallee::cell`).
    let facts = quench_runtime::staged::call::CellFacts::of_word(unsafe { (*(callee.cell as *const entry_cell::EntryCell)).facts.get() });
    facts.registers == registers && facts.room == registers && u64::from(facts.entry) == quench_runtime::staged::call::ENTRY_ACTIVATES
}

/// The `QUENCH_TRACE=aot-lanes` diagnostic for a lane body the image has but this VM cannot publish.
fn refuse_lane(function: u32, why: &str) {
    if quench_runtime::trace::Channel::AotLanes.on() {
        eprintln!("aot-lanes: install function {function}: image lane refused, {why}");
    }
}

#[quench_macros::ext(CompiledAheadImagesVm)]
impl Vm {
    /// `function`'s body from the loaded images for this program, published as its lane body,
    /// linked to this VM at the address it runs. An image's base body installs nothing (`fbits9`):
    /// a function whose lane body this VM cannot take runs its records in the general loop.
    pub(crate) fn install_ahead_body(&mut self, unit: super::admission::CompiledUnit, program: &ResidualProgram, function: u32, entry: Option<u32>, generic: &[LaneExit]) -> Option<Rc<PublishedLaneMethod>> {
        // The unit is the gate's proof that the trace range admits `function` (`admission::ahead_unit`).
        let _ = unit;
        // Native code reads slot representations, which the runtime keeps from its first compile on.
        self.keep_slot_representations();
        let traced = quench_runtime::trace::Channel::AotLanes.on();
        if entry.is_some() || !generic.is_empty() {
            if traced { eprintln!("aot-lanes: install function {function}: refused, entry {entry:?}, {} generic leaves", generic.len()); }
            return None;
        }
        let Some(images) = self.programs.ahead_images(self.active_program, program) else {
            if traced { eprintln!("aot-lanes: install function {function}: refused, no image for the program"); }
            return None;
        };
        let lanes_allowed = super::admission::within_trace_range(quench_runtime::trace::Dev::AotLanes, function);
        // An image's base body does not run a lane activation (`fbits9`), so it installs nothing:
        // a function without a usable lane body runs its records in the general loop, which
        // resumes the lane at the next record it can take (`ExecutionMode::NativeOnly` admits
        // that one step, `Vm::may_run_in_lane`).
        let Some(method) = lanes_allowed.then(|| self.ahead_lane_method(program, &images, function)).flatten() else {
            if traced { eprintln!("aot-lanes: install function {function}: no lane body, its records step"); }
            return None;
        };
        if quench_runtime::trace::Channel::AotLanes.on() {
            let name = program.functions.get(function as usize)
                .and_then(|f| f.header().name.or(f.header().inferred_name))
                .and_then(|atom| program.atoms.get(atom as usize)).map_or("", |name| &name[..]);
            eprintln!("aot-lanes: install function {function} ({name}): lane body");
        }
        let published = { let (mut facts, programs) = self.fact_watch(); programs.publish_lane_method(method, &mut facts) }?;
        let linked_live: &LiveImage = images.linked.as_ref()?;
        for (key, callee) in self.ahead_callees(program, &images.image, function) { linked_live.calls.bind(key, callee, self); }
        Some(published)
    }

    /// `function`'s lane body from `images`, linked at the address it will run.
    fn ahead_lane_method(&mut self, program: &ResidualProgram, images: &AheadImages, function: u32) -> Option<LaneMethod> {
        let refused = |why: &str| refuse_lane(function, why);
        let Some(linked_live) = images.linked.as_ref() else { refused("the image did not link"); return None };
        let Some(lane) = images.image.functions.get(function as usize).and_then(|function| function.lane.as_ref()) else {
            refused("the image holds no lane body for it"); return None;
        };
        let fault_poll = lane.fault_poll;
        let Some(unit) = unit_of(program, function, images.facts.get_or_init(|| quench_runtime::compile::aot_facts::analyze_as(program, images.image.world))) else {
            refused("no unit"); return None;
        };
        // The body is linked where it will run: its page- and displacement-relative fields are its
        // address's, so the placement comes first (`live::link_lane_at`). The value table follows
        // the code in the same block.
        let value_bytes = live::value_words(&images.image) * std::mem::size_of::<u64>();
        let stubs = live::stub_bytes_in_lane(&lane.code);
        let mut roots = None;
        let memory = super::code_heap::publishing("u-jit-lane", function as usize, || {
            super::code_heap::CodeMemory::assemble(lane.code.code.len() + value_bytes + stubs, |address, out| {
                roots = Some(live::link_lane_at(&images.image, self, linked_live, function, address.addr() as u64, out)?);
                Ok(())
            })
        });
        let memory = match memory {
            Ok(memory) => memory,
            Err(error) => { refuse_lane(function, &format!("link_lane_at: {error}")); return None; }
        };
        let roots = roots.unwrap_or_default();
        // The heap values the code names are its roots, which its publication keeps alive; the
        // Number slots its guessed field reads read as tagged words retire it where they widen to
        // raw doubles (`fields`).
        let slots = guard_dependencies(&images.image, self, &lane.code);
        // The leaves the image's lowering laid out, each inlined one with the values it stores.
        let exits = ExitTable { sigs: lane.exits.sigs.clone(), sites: lane.exits.sites.clone(),
            crossings: (0..lane.exits.sites.len()).map(|_| std::cell::Cell::new(0)).collect() };
        let facts = lane_compile::LaneMethodFacts {
            unit, stable: lane_region::StableFacts { roots, slots, ..Default::default() }, fields: Vec::new(),
            direct: Box::new([]), closed: false, function, invalidation: linked_live.invalidation.clone(),
            tiers: self.programs.tiers(), fault_poll, exits, summary: None, clif: None,
        };
        let method = lane_compile::finish_lane_method(memory, None, None, facts);
        if method.is_none() { refuse_lane(function, "finish_lane_method"); }
        method
    }

    /// The callees of `function`'s proven call sites (`AotImage::calls`) that bind now, each by its
    /// record's key: the closure each site resolved of the function the closed world proves it calls
    /// (`Vm::proven_callee`), where the site's row admits it (`admits_proven`). A site that binds
    /// nothing yet keeps refusing its calls to the VM's, whose call binds it (`rebind_ahead_call`).
    fn ahead_callees(&self, program: &ResidualProgram, image: &AotImage, function: u32) -> Vec<(usize, DirectCallee)> {
        image.calls.iter().enumerate().filter(|(_, site)| site.function == function).filter_map(|(key, site)| {
            let direct = self.proven_callee(program, (function, site.pc), site.callee)?;
            admits_proven(program, site.callee, &direct).then_some((key, direct))
        }).collect()
    }

    /// Binds the record of the proven call at `pc` of `function` to `target`,
    /// the callee the VM's call of the site just resolved, where the record names another closure: the
    /// record's guard missed because the program made a new closure of the proven function, and its
    /// rows call this one by its entry from now on (`BoundCalls::bind`). Nothing where no loaded image
    /// proves the call, or `target` is no closure of the proven function.
    pub(crate) fn rebind_ahead_call(&mut self, program: &ResidualProgram, (function, pc): (u32, u32), (argc, target): (u16, crate::dispatch_fast::ScriptCallTarget)) {
        let why = self.rebound_ahead_call(program, (function, pc), (argc, target));
        // The common answers (no proof of the site, the record already names the callee) are not traced: every helper call makes one.
        if !matches!(why, "unproven-site" | "already-named") && quench_runtime::trace::Channel::BodyLeave.on() { eprintln!("QUENCH_REBIND function={function} pc={pc} callee={} {why}", target.function); }
    }

    /// `rebind_ahead_call`, answering what it did.
    fn rebound_ahead_call(&mut self, program: &ResidualProgram, (function, pc): (u32, u32), (argc, target): (u16, crate::dispatch_fast::ScriptCallTarget)) -> &'static str {
        let Some(images) = self.programs.ahead_images(self.active_program, program) else { return "no-image" };
        let Some(live) = images.linked.as_ref() else { return "unlinked" };
        let Some(key) = live.calls.key((function, pc)) else { return "unproven-site" };
        let facts = images.facts.get_or_init(|| quench_runtime::compile::aot_facts::analyze_as(program, images.image.world));
        if facts.function_facts(function).call_target(pc) != Some(target.function) { return "other-function" }
        let Some(views) = self.programs.views_in_lane(self.active_program) else { return "no-views" };
        let Some(view) = views.get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view) else { return "no-view" };
        let Some(mut direct) = crate::dispatch_fast::direct_callee_in_lane(self, view, function, pc as usize, argc, target) else { return "no-direct" };
        (direct.method, direct.body) = (None, None);
        if live.calls.names(key, direct.callee) { return "already-named" }
        if !admits_proven(program, target.function, &direct) { return "not-admitted" }
        live.calls.bind(key, direct, self);
        "bound"
    }


    /// Takes `images`, links them into this VM, and keeps them for the program they were
    /// compiled from, whenever it runs.
    pub fn load_ahead_images(&mut self, mut images: AheadImages) {
        let linked = live::install(&images.image, self);
        if let Err(why) = &linked && quench_runtime::trace::Channel::AotLanes.on() { eprintln!("aot-lanes: image did not link: {why}"); }
        images.linked = linked.ok();
        self.programs.add_ahead_images(images);
    }

    /// Keeps the image the system linker placed at `table` for the program it was compiled from
    /// (`AheadImages::of_linked`), once: `program`'s functions lower now, before any frame runs
    /// them, as the compile lowered them (`lower_native_js_module_for`), so its PCs are the image's.
    pub(crate) fn load_linked_image(&mut self, program: &ResidualProgram, table: &live::LinkedTable) -> Result<(), String> {
        if self.programs.has_linked_image(table) { return Ok(()); }
        for index in 0..program.function_count() { program.lowered_function(index); }
        self.programs.add_ahead_images(AheadImages::of_linked(table)?);
        // The image's bodies are compiled already, so the gate that keeps runtime lane-body
        // compiles off on AArch64 (`enable_js_lane_methods`) does not apply to them.
        self.programs.enable_ahead_lane_bodies();
        Ok(())
    }
}
