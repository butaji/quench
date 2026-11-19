//! The ahead-of-time image of a program as a loadable file. `AheadImages` holds the program's
//! lowered `AotImage` (`aot_image::lower`), links it into the VM that loads it
//! (`aot_image::live::install`), and reads and writes the file (`aot_image::file`). A recording
//! bundles the images of one run into one file.
use super::*;
use crate::vm::jit_backend::relocatable_code::RelocatableCode;
use crate::vm::aot_image::{LaneImage, base::{BaseImage, NO_ENTRY}, symbols::{Symbol, SymbolTable}};
use crate::compile::aot_facts::AotFacts;
use super::lane_unit::Origin;
use crate::vm::aot_image::{self, AotImage, ImageTarget, file::Words, live::{self, LiveImage}};

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
    let body = program.function_get(function as usize)?;
    let unit = lane_unit::unit(program, function, body, &|caller, pc| closed_targets(program, facts, (caller, pc)), &|callee| program.function_get(callee as usize))?;
    unit.frames.iter().all(|frame| frame.recovery == lane_unit::CallRecovery::RestartCall).then_some(unit)
}

/// The code of the lane body `unit` lowers to under `facts` (`LaneHelpers::Ahead`), and whether
/// it polls for faults; `None` where it gives no native record.
fn lowered_code(program: &ResidualProgram, unit: &lane_unit::Unit, function: u32, (facts, global_reads): (VmFacts<'_>, &crate::vm::global_facts::GlobalReads))
    -> Option<(RelocatableCode, bool)> {
    let mut lowered = lane_region::lower_body(program, function, unit, LaneHelpers::Ahead, &[],
        global_reads, facts, &[], &BTreeMap::new(), &[], None, &[])?;
    if lowered.region.native == 0 { return None; }
    let native = std::mem::replace(&mut lowered.region.function, ir::Function::new());
    let isa = crate::vm::jit_backend::target_isa()?;
    let code = crate::vm::jit_backend::compile_function(native, &*isa, Some(crate::vm::compiled::tier_policy::LANE_BODY_MAX_CODE_BYTES), |compiled, function| {
        RelocatableCode::extract(compiled, function).ok_or(NativeCompileRejection::Relocations)
    }).ok()?;
    Some((code, lowered.region.fault_poll))
}

/// The lane body of `function` for an AOT image under the closed-world `facts`, its leaves
/// continuing at `base`'s entries (`join`): every leave's target is interned in `symbols`. The
/// whole-program values of `frozen` fold at their reads, each read checked where it is made
/// (`GlobalRead::Frozen`, `VmFacts::frozen_fields`), as the JIT's frozen division folds them.
/// `None` where it gives no native record, or polls for faults: the lowering does not report
/// the poll's offset, so the base body runs such a function.
pub(in crate::vm) fn closed_lane_image(program: &ResidualProgram, function: u32, (facts, frozen): (&AotFacts, &ClosedFrozen),
    base: &BaseImage, symbols: &mut SymbolTable,
) -> Option<LaneImage> {
    let unit = unit_of(program, function, facts)?;
    let targets = leave_targets(&unit, function, base, symbols);
    let global_reads = super::frozen::frozen_unit_reads(program, &unit, function, &frozen.globals);
    let facts = VmFacts { ahead: Some(lane_region::AheadLeaves { targets: &targets }), closed: Some(facts),
        frozen_fields: Some(&frozen.fields), ..VmFacts::STATIC };
    let (code, fault_poll) = lowered_code(program, &unit, function, (facts, &global_reads))?;
    (!fault_poll).then_some(LaneImage { code, fault_poll: None })
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
    /// The program's closed-world facts, which shaped its lane bodies' units (`unit_of`): the
    /// compile's, or derived once again from the program where the image was read from a file.
    facts: std::cell::OnceCell<AotFacts>,
}

/// The images of each program stored while recording (`start_recording`); the entry may run
/// on a worker thread, so this is process-wide.
static RECORDED: std::sync::Mutex<Option<Vec<AheadImages>>> = std::sync::Mutex::new(None);

fn recorded() -> std::sync::MutexGuard<'static, Option<Vec<AheadImages>>> { RECORDED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) }

impl AheadImages {
    /// The image of `program`, lowered under its closed-world facts for the baseline ISA.
    pub fn compile(program: &ResidualProgram) -> Result<Self, String> {
        let facts = crate::compile::aot_facts::analyze(program);
        let image = aot_image::lower(program, &facts, &ImageTarget::default())?;
        Ok(Self { facts: std::cell::OnceCell::from(facts), ..Self::of(image) })
    }

    fn of(image: AotImage) -> Self {
        Self { image, known: Default::default(), linked: None, facts: std::cell::OnceCell::new() }
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

    /// The image `to_bytes` wrote; an error names what the file lacks.
    pub fn from_bytes(file: &[u8]) -> Result<Self, String> { Ok(Self::of(aot_image::file::from_bytes(file)?)) }

    /// Starts collecting the images of every program this thread stores (`finish_recording`).
    pub fn start_recording() { *recorded() = Some(Vec::new()); }

    /// Adds `program`'s image while recording; a program that does not lower adds none.
    pub(in crate::vm) fn record(program: &ResidualProgram) {
        if recorded().is_none() { return; }
        let Ok(images) = Self::compile(program) else { return };
        if !images.is_empty() { if let Some(all) = recorded().as_mut() { all.push(images); } }
    }

    /// What was recorded since `start_recording`, which stops.
    pub fn finish_recording() -> Vec<AheadImages> { recorded().take().unwrap_or_default() }

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

/// Whether `callee`'s entry cell holds the facts of a plain activation of `registers` registers, the
/// facts an image's direct row is planned with (`callees::proven_facts`).
fn plain_cell(callee: &DirectCallee, registers: u16) -> bool {
    // SAFETY: a direct callee's cell is its function's live entry cell (`DirectCallee::cell`).
    let facts = crate::staged::call::CellFacts::of_word(unsafe { (*(callee.cell as *const entry_cell::EntryCell)).facts.get() });
    facts.registers == registers && facts.room == registers && u64::from(facts.entry) == crate::staged::call::ENTRY_ACTIVATES
}

impl<H: Host> Vm<H> {
    /// `function`'s lane body linked to this VM and published as its lane body, if the loaded
    /// images hold one for this program.
    pub(in crate::vm) fn install_ahead_body(&mut self, program: &ResidualProgram, function: u32, entry: Option<u32>, generic: &[LaneExit]) -> Option<Rc<PublishedLaneMethod>> {
        if entry.is_some() || !generic.is_empty() { return None; }
        let images = self.programs.ahead_images(self.active_program, program)?;
        let linked_live: &LiveImage = images.linked.as_ref()?;
        let fault_poll = images.image.functions.get(function as usize)?.lane.as_ref()?.fault_poll.is_some();
        let unit = unit_of(program, function, images.facts.get_or_init(|| crate::compile::aot_facts::analyze(program)))?;
        let linked = live::link_lane(&images.image, self, linked_live, function)?;
        let region = lane_region::LoweredRegion { number_result: None, function: ir::Function::new(), native: 1,
            fault_poll, hoists_arrays: false, stable: Default::default() };
        let (keys, direct): (Vec<usize>, Vec<DirectCallee>) = self.ahead_callees(program, &images.image, function).into_iter().unzip();
        let plan = lane_compile::LanePlan { unit, lowered: lane_region::LoweredLane { region, clif: None, scalar: None }, fields: Vec::new(),
            direct: direct.into_boxed_slice(), closed: false, function, invalidation: linked_live.invalidation.clone(), budget: 0, tiers: self.programs.tiers() };
        let method = plan.finish(&linked, None)?;
        let published = self.programs.publish_lane_method(method, &mut self.heap, self.method_cache_atoms.readers())?;
        // The body owns the descriptors its sites' records name, and their binding retires with it (`live::bind_call`).
        let owned = published.lease()?;
        for (key, callee) in keys.into_iter().zip(owned.direct.iter()) { live::bind_call(linked_live, key, callee, &published)?; }
        Some(published)
    }

    /// The callees of `function`'s proven call sites (`AotImage::calls`) that bind now, each by its
    /// record's key: the closure each site resolved of the function the closed world proves it calls
    /// (`Vm::proven_callee`), where it binds `this` as the site's row was planned (`ThisForm`) and its
    /// cell holds the facts of a plain activation of its window (as `DirectFacts::describes` admits a JIT
    /// plan's). A site that binds nothing yet keeps refusing its calls to the helper.
    fn ahead_callees(&self, program: &ResidualProgram, image: &AotImage, function: u32) -> Vec<(usize, DirectCallee)> {
        image.calls.iter().enumerate().filter(|(_, site)| site.function == function).filter_map(|(key, site)| {
            let body = program.function_get(site.callee as usize)?;
            let direct = self.proven_callee(program, (function, site.pc), site.callee)?;
            let this = super::base_region::callees::ThisForm::of(body);
            (direct.sloppy.is_some() == this.sloppy && direct.lexical == this.lexical && plain_cell(&direct, body.registers)).then_some((key, direct))
        }).collect()
    }

    /// Takes `images`, links them into this VM, and keeps them for the program they were
    /// compiled from, whenever it runs.
    pub fn load_ahead_images(&mut self, mut images: AheadImages) {
        images.linked = live::install(&images.image, self).ok();
        self.programs.add_ahead_images(images);
    }
}
