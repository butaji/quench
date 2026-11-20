//! The ahead-of-time image of a program as a loadable file. `AheadImages` holds the program's
//! lowered `AotImage` (`aot_image::lower`), links it into the VM that loads it
//! (`aot_image::live::install`), and reads and writes the file (`aot_image::file`). A recording
//! bundles the images of one run into one file.
use super::*;
use std::collections::BTreeMap;
use crate::vm::jit_backend::relocatable_code::RelocatableCode;
use crate::vm::aot_image::{LaneImage, base::{BaseImage, NO_ENTRY}, symbols::{Symbol, SymbolTable}};
use crate::compile::aot_facts::AotFacts;
use super::lane_unit::Origin;
use crate::vm::aot_image::{self, AotImage, ImageTarget, file::Words, live::{self, LiveImage}};
#[path = "ahead_images/constructions.rs"] mod constructions;

/// Names the build whose machine code and layouts the images are (`build.rs`); a file from another build is rejected.
pub(crate) const AHEAD_BUILD_ID: u64 = match u64::from_str_radix(env!("QUENCH_BUILD_ID"), 16) { Ok(id) => id, Err(_) => 0 };

/// What names a program: a hash of its source text and every function's registers and opcodes
/// (atom numbers are per VM, so no operand), which an image is valid for and no other (plus the helper count the code was linked against; it is a validity key, never a reason to treat a program specially).
pub(crate) fn ahead_program_fingerprint(program: &ResidualProgram) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mix = |hash: u64, bytes: &[u8]| bytes.iter().fold(hash, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME));
    let mut hash = mix(FNV_OFFSET, &(super::LaneHelper::COUNT as u32).to_le_bytes());
    hash = mix(hash, program.source_text().unwrap_or_default().as_bytes());
    for index in 0..program.function_count() {
        let function = program.function(index);
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
/// default budget); `None` where a frame it inlines would leave by making the callee's activation,
/// which no base body continues (`leave_targets`).
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
fn lowered_code(program: &ResidualProgram, unit: &lane_unit::Unit, function: u32, (facts, global_reads): (VmFacts<'_>, &crate::vm::global_facts::GlobalReads))
    -> Result<(RelocatableCode, bool), &'static str> {
    let mut lowered = lane_region::lower_body(program, function, unit, LaneHelpers::Ahead, &[],
        global_reads, facts, &[], &BTreeMap::new(), &[], None, &[]).ok_or("the lane lowering declines")?;
    if lowered.region.native == 0 { return Err("no native record"); }
    let native = std::mem::replace(&mut lowered.region.function, ir::Function::new());
    let isa = crate::vm::jit_backend::ahead_lane_isa().ok_or("no lane ISA")?;
    let code = crate::vm::jit_backend::compile_function(native, &*isa, Some(crate::vm::compiled::tier_policy::LANE_BODY_MAX_CODE_BYTES), |compiled, function| {
        RelocatableCode::extract(compiled, function).ok_or(NativeCompileRejection::Relocations)
    }).map_err(|rejection| match rejection {
        NativeCompileRejection::CodeSizeBudget => "code past the lane body budget",
        NativeCompileRejection::Relocations => "a relocation the image cannot name",
        _ => "Cranelift rejects the code",
    })?;
    Ok((code, lowered.region.fault_poll))
}

/// The lane body of `function` for an AOT image under the closed-world `facts`, its leaves
/// continuing at `base`'s entries (`join`): every leave's target is interned in `symbols`. The
/// whole-program values of `frozen` fold at their reads, each read checked where it is made
/// (`GlobalRead::Frozen`, `VmFacts::frozen_fields`), as the JIT's frozen division folds them. A
/// construction the closed world proves keeps its receiver as its fields (`constructions`). A call
/// the closed world proves reads its callee's words from the base body's record (`proven_calls`).
/// `None` where it gives no native record, or polls for faults: the lowering does not report
/// the poll's offset, so the base body runs such a function.
pub(in crate::vm) fn closed_lane_image(program: &ResidualProgram, function: u32, (facts, frozen): (&AotFacts, &ClosedFrozen),
    (base, calls): (&BaseImage, &[aot_image::CallSite]), symbols: &mut SymbolTable,
) -> Result<LaneImage, &'static str> {
    let unit = unit_or_refusal(program, function, facts)?;
    // A VM installs no lane body that inlines a frame (`ahead_lane_method`): its leaves inside one have no store list.
    if !unit.frames.is_empty() { return Err("inlines a call (not installable)"); }
    let targets = leave_targets(&unit, function, base, symbols);
    let proven = proven_calls(program, &unit, function, calls, symbols);
    let plans = constructions::construct_plans(program, &unit, function, facts, symbols);
    let plan_of = |caller: u32, pc: u32| plans.get(&(caller, pc)).cloned();
    let global_reads = super::frozen::frozen_unit_reads(program, &unit, function, &frozen.globals);
    let facts = VmFacts { ahead: Some(lane_region::AheadLeaves { targets: &targets, calls: &proven }), closed: Some(facts),
        frozen_fields: Some(&frozen.fields), construct_plans: Some(&plan_of), ..VmFacts::STATIC };
    let (code, fault_poll) = lowered_code(program, &unit, function, (facts, &global_reads))?;
    if fault_poll { return Err("polls for faults"); }
    Ok(LaneImage { code, fault_poll: None })
}

/// By unit PC, the caller calls the closed world proves that the base body planned a record for
/// (`calls`, its `AotImage::calls`): the lane body calls each through the same record, which the
/// loader binds (`BoundCalls::bind`) and the lane reads by `Symbol::CallRecord`. An arrow is left
/// to the VM's call, as the JIT's lane leaves it (`direct_callee_in_lane`): it binds no receiver.
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

/// Each unit PC's leave target as the base body's `Symbol::BasePc`, where the base body enters it: a
/// caller record, or for a PC of an inlined callee the caller's call that inlined it, which restarts
/// (every frame's recovery is `RestartCall`, `unit_of`: the callee ran no effect before it leaves).
fn leave_targets(unit: &lane_unit::Unit, function: u32, base: &BaseImage, symbols: &mut SymbolTable) -> Box<[Option<(u32, u32)>]> {
    let caller_call = |mut frame: usize| {
        while let Some(parent) = unit.frames[frame].parent { frame = parent; }
        unit.frames[frame].call
    };
    unit.origins.iter().map(|origin| {
        let record = match *origin {
            Origin::Caller(record) => record,
            Origin::Inlined { frame, .. } => caller_call(frame),
        };
        base.pc_offsets.get(record as usize).is_some_and(|at| *at != NO_ENTRY)
            .then(|| symbols.intern(Symbol::BasePc { function, pc: record }))
    }).collect()
}

/// A program's ahead-of-time image, and its link into this process once a VM loads it.
pub struct AheadImages {
    image: AotImage,
    /// The programs (by number) checked against it, and whether each is it.
    known: std::cell::RefCell<Vec<(u32, bool)>>,
    /// The image placed in the loading VM's executable memory; `None` where it did not link.
    linked: Option<LiveImage>,
    /// The address of the entry table the system linker placed it at, for an image of the executable (`of_linked`).
    placed: Option<usize>,
    /// The program's closed-world facts, which shaped its lane bodies' units (`unit_of`): the
    /// compile's, or derived once again from the program where the image was read from a file.
    facts: std::cell::OnceCell<AotFacts>,
}

/// The images of each program stored while recording (`start_recording`); the entry may run
/// on a worker thread, so this is process-wide. It holds the lowered images alone: a linked
/// image belongs to the VM (and thread) that loaded it.
static RECORDED: std::sync::Mutex<Option<Vec<AotImage>>> = std::sync::Mutex::new(None);

fn recorded() -> std::sync::MutexGuard<'static, Option<Vec<AotImage>>> { RECORDED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) }

impl AheadImages {
    /// The image of `program`, lowered under its closed-world facts for the baseline ISA.
    pub fn compile(program: &ResidualProgram) -> Result<Self, String> {
        let facts = crate::compile::aot_facts::analyze(program);
        let image = aot_image::lower(program, &facts, &ImageTarget::default())?;
        Ok(Self { facts: std::cell::OnceCell::from(facts), ..Self::of(image) })
    }

    fn of(image: AotImage) -> Self {
        Self { image, known: Default::default(), linked: None, placed: None, facts: std::cell::OnceCell::new() }
    }

    /// Whether this is the image of `program`, the program numbered `id`; each program is
    /// fingerprinted once.
    pub(in crate::vm) fn is_of(&self, id: u32, program: &ResidualProgram) -> bool {
        let mut known = self.known.borrow_mut();
        if let Some((_, of)) = known.iter().find(|(known, _)| *known == id) { return *of; }
        let of = ahead_program_fingerprint(program) == self.image.fingerprint;
        known.push((id, of));
        of
    }

    /// How many functions the image has.
    pub fn len(&self) -> usize { self.image.functions.len() }

    /// Whether the image has no functions.
    pub fn is_empty(&self) -> bool { self.image.functions.is_empty() }

    /// The file of this image (`from_bytes` reads it back).
    pub fn to_bytes(&self) -> Vec<u8> { aot_image::file::to_bytes(&self.image) }

    /// The image the system linker placed in this executable (`aot_image::object`), as its entry
    /// table lists it: its file read back, its code and records used where they are placed.
    pub(crate) fn of_linked(table: &live::LinkedTable) -> Result<Self, String> {
        let image = aot_image::file::from_bytes(table.file)?;
        let linked = live::linked(&image, table)?;
        Ok(Self { linked: Some(linked), placed: Some(table.entries.as_ptr().addr()), ..Self::of(image) })
    }

    /// Whether this is the image the system linker placed at `table`'s entries.
    pub(crate) fn is_linked_at(&self, table: &live::LinkedTable) -> bool {
        self.placed == Some(table.entries.as_ptr().addr())
    }

    /// The image `to_bytes` wrote; an error names what the file lacks.
    pub fn from_bytes(file: &[u8]) -> Result<Self, String> { Ok(Self::of(aot_image::file::from_bytes(file)?)) }

    /// Starts collecting the images of every program this thread stores (`finish_recording`).
    pub fn start_recording() { *recorded() = Some(Vec::new()); }

    /// Adds `program`'s image while recording; a program that does not lower adds none.
    pub(in crate::vm) fn record(program: &ResidualProgram) {
        if recorded().is_none() { return; }
        let Ok(images) = Self::compile(program) else { return };
        if !images.is_empty() { if let Some(all) = recorded().as_mut() { all.push(images.image); } }
    }

    /// What was recorded since `start_recording`, which stops.
    pub fn finish_recording() -> Vec<AheadImages> { recorded().take().unwrap_or_default().into_iter().map(Self::of).collect() }

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
    let facts = crate::staged::call::CellFacts::of_word(unsafe { (*(callee.cell as *const entry_cell::EntryCell)).facts.get() });
    facts.registers == registers && facts.room == registers && u64::from(facts.entry) == crate::staged::call::ENTRY_ACTIVATES
}

impl<H: Host> Vm<H> {
    /// `function`'s body from the loaded images for this program, published as its lane body: its
    /// lane body linked to this VM, or where it has none (or none this VM can take) its base body
    /// where the image placed it.
    pub(in crate::vm) fn install_ahead_body(&mut self, program: &ResidualProgram, function: u32, entry: Option<u32>, generic: &[LaneExit]) -> Option<Rc<PublishedLaneMethod>> {
        if entry.is_some() || !generic.is_empty() { return None; }
        let images = self.programs.ahead_images(self.active_program, program)?;
        let (method, kind) = match self.ahead_lane_method(program, &images, function) {
            Some(method) => (method, "lane"),
            None => (Self::ahead_base_method(program, &images, function)?, "base"),
        };
        if crate::trace::Channel::AotLanes.on() { eprintln!("aot-lanes: install function {function}: {kind} body"); }
        let published = self.programs.publish_lane_method(method, &mut self.heap, self.method_cache_atoms.readers())?;
        let linked_live: &LiveImage = images.linked.as_ref()?;
        for (key, callee) in self.ahead_callees(program, &images.image, function) { linked_live.calls.bind(key, callee, &mut self.heap); }
        Some(published)
    }

    /// `function`'s lane body from `images`, linked to this VM.
    fn ahead_lane_method(&mut self, program: &ResidualProgram, images: &AheadImages, function: u32) -> Option<LaneMethod> {
        let linked_live: &LiveImage = images.linked.as_ref()?;
        let fault_poll = images.image.functions.get(function as usize)?.lane.as_ref()?.fault_poll.is_some();
        let unit = unit_of(program, function, images.facts.get_or_init(|| crate::compile::aot_facts::analyze(program)))?;
        // An image has no lowering to ask which registers a leave inside an inlined frame stores.
        if !unit.frames.is_empty() { return None; }
        let (linked, roots) = live::link_lane(&images.image, self, linked_live, function)?;
        // The heap values the code names are its roots, which its publication keeps alive.
        let region = lane_region::LoweredRegion { number_result: None, function: ir::Function::new(), native: 1,
            fault_poll, hoists_arrays: false, stable: lane_region::StableFacts { roots, ..Default::default() }, entry_types: Default::default(), summary: None,
            exits: ExitTable::of_unit(&unit, |_, _| Vec::new()) };
        let plan = lane_compile::LanePlan { unit, lowered: lane_region::LoweredLane { region, clif: None, scalar: None, proven: None }, fields: Vec::new(),
            direct: Box::new([]), closed: false, function, invalidation: linked_live.invalidation.clone(), budget: 0, tiers: self.programs.tiers() };
        plan.finish(&linked, None, None)
    }

    /// `function`'s base body from `images`, where they are placed (`LiveImage::bases`).
    fn ahead_base_method(program: &ResidualProgram, images: &AheadImages, function: u32) -> Option<LaneMethod> {
        let linked_live: &LiveImage = images.linked.as_ref()?;
        let base = &images.image.functions.get(function as usize)?.base;
        let registers = program.function_get(function as usize)?.registers;
        Some(LaneMethod::of_linked_base(*linked_live.bases.get(function as usize)?, registers, base.scratch as usize, linked_live.invalidation.clone()))
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
    pub(in crate::vm) fn rebind_ahead_call(&mut self, program: &ResidualProgram, (function, pc): (u32, u32), (argc, target): (u16, super::super::dispatch_fast::ScriptCallTarget)) {
        let Some(images) = self.programs.ahead_images(self.active_program, program) else { return };
        let Some(live) = images.linked.as_ref() else { return };
        let Some(key) = live.calls.key((function, pc)) else { return };
        let facts = images.facts.get_or_init(|| crate::compile::aot_facts::analyze(program));
        if facts.function_facts(function).call_target(pc) != Some(target.function) { return; }
        let Some(views) = self.programs.views_in_lane(self.active_program) else { return };
        let Some(view) = views.get(function as usize).and_then(super::super::program_store::derived_lane_view) else { return };
        let Some(mut direct) = super::super::dispatch_fast::direct_callee_in_lane(self, view, function, pc as usize, argc, target) else { return };
        (direct.method, direct.body) = (None, None);
        if live.calls.names(key, direct.callee) || !admits_proven(program, target.function, &direct) { return; }
        live.calls.bind(key, direct, &mut self.heap);
    }

    /// Takes `images`, links them into this VM, and keeps them for the program they were
    /// compiled from, whenever it runs.
    pub fn load_ahead_images(&mut self, mut images: AheadImages) {
        images.linked = live::install(&images.image, self).ok();
        self.programs.add_ahead_images(images);
    }

    /// Keeps the image the system linker placed at `table` for the program it was compiled from
    /// (`AheadImages::of_linked`), once: `program`'s functions lower now, before any frame runs
    /// them, as the compile lowered them (`lower_native_js_module_for`), so its PCs are the image's.
    pub(in crate::vm) fn load_linked_image(&mut self, program: &ResidualProgram, table: &live::LinkedTable) -> Result<(), String> {
        if self.programs.has_linked_image(table) { return Ok(()); }
        for index in 0..program.function_count() { program.lowered_function(index); }
        self.programs.add_ahead_images(AheadImages::of_linked(table)?);
        Ok(())
    }
}
