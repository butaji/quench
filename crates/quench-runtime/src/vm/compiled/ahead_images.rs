//! The ahead-of-time image of a program as a loadable file. `AheadImages` holds the program's
//! lowered `AotImage` (`aot_image::lower`), links it into the VM that loads it
//! (`aot_image::live::install`), and reads and writes the file (`aot_image::file`). A recording
//! bundles the images of one run into one file.
use super::*;
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

/// A program's ahead-of-time image, and its link into this process once a VM loads it.
pub struct AheadImages {
    image: AotImage,
    /// The programs (by number) checked against it, and whether each is it.
    known: std::cell::RefCell<Vec<(u32, bool)>>,
    /// The image placed in the loading VM's executable memory; `None` where it did not link.
    linked: Option<LiveImage>,
}

/// The images of each program stored while recording (`start_recording`); the entry may run
/// on a worker thread, so this is process-wide.
static RECORDED: std::sync::Mutex<Option<Vec<AheadImages>>> = std::sync::Mutex::new(None);

fn recorded() -> std::sync::MutexGuard<'static, Option<Vec<AheadImages>>> { RECORDED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) }

impl AheadImages {
    /// The image of `program`, lowered under its closed-world facts for the baseline ISA.
    pub fn compile(program: &ResidualProgram) -> Result<Self, String> {
        let facts = crate::compile::aot_facts::analyze(program);
        let target = ImageTarget { isa: crate::vm::jit_backend::IsaBaseline::default() };
        Ok(Self::of(aot_image::lower(program, &facts, target)?))
    }

    fn of(image: AotImage) -> Self {
        Self { image, known: Default::default(), linked: None }
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

impl<H: Host> Vm<H> {
    /// `function`'s lane body linked to this VM and published as its lane body, if the loaded
    /// images hold one for this program.
    pub(in crate::vm) fn install_ahead_body(&mut self, program: &ResidualProgram, function: u32, entry: Option<u32>, generic: &[LaneExit]) -> Option<Rc<PublishedLaneMethod>> {
        if entry.is_some() || !generic.is_empty() { return None; }
        let images = self.programs.ahead_images(self.active_program, program)?;
        let linked_live: &LiveImage = images.linked.as_ref()?;
        let fault_poll = images.image.functions.get(function as usize)?.lane.as_ref()?.fault_poll.is_some();
        let linked = live::link_lane(&images.image, self, linked_live, function)?;
        let body = program.function_get(function as usize)?;
        let unit = lane_unit::with_budget(program, function, body, 0, &|_, _| Vec::new(), &|callee| program.function_get(callee as usize))?;
        let region = lane_region::LoweredRegion { number_result: None, function: ir::Function::new(), native: 1,
            fault_poll, hoists_arrays: false, stable: Default::default() };
        let plan = lane_compile::LanePlan { unit, lowered: lane_region::LoweredLane { region, clif: None, scalar: None }, fields: Vec::new(),
            direct: Box::new([]), closed: false, function, invalidation: crate::code_dependencies::Invalidation::default(), budget: 0, tiers: self.programs.tiers() };
        let method = plan.finish(&linked, None)?;
        self.programs.publish_lane_method(method, &mut self.heap, self.method_cache_atoms.readers())
    }

    /// Takes `images`, links them into this VM, and keeps them for the program they were
    /// compiled from, whenever it runs.
    pub fn load_ahead_images(&mut self, mut images: AheadImages) {
        images.linked = live::install(&images.image, self).ok();
        self.programs.add_ahead_images(images);
    }
}
