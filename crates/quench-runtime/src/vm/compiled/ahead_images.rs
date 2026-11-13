//! Lane bodies compiled ahead of time (`LaneHelpers::Ahead`): the program alone, no fact a run
//! observed, lowered once to position-independent code whose run-time addresses are symbols.
//! The images are a file (`AheadImages::to_bytes`); a VM that loads it links each image to its
//! own helper table and publishes it where a compile of that function would (`install`).
use super::*;
use crate::vm::jit_backend::relocatable_code::{RelocatableCode, Relocation};
use std::collections::BTreeMap;

/// The lane body of one function: its code, and what its lowering proved about the entry ABI.
#[derive(Debug)]
struct Image {
    fault_poll: bool,
    code: RelocatableCode,
    /// The text of each atom the code names, by the number it names it with.
    atoms: Vec<(u32, String)>,
}

/// A program's ahead-of-time lane bodies by function.
#[derive(Debug, Default)]
pub struct AheadImages {
    /// The program they are of (`fingerprint`).
    program: u64,
    /// The programs (by number) checked against it, and whether each is it.
    known: std::cell::RefCell<Vec<(u32, bool)>>,
    /// By function, ascending.
    images: Vec<(u32, Image)>,
}

const MAGIC: &[u8; 8] = b"QAHEAD\0\x02";

/// Names the build whose machine code and layouts the images are (`build.rs`); a file from another build is rejected.
const BUILD_ID: u64 = match u64::from_str_radix(env!("QUENCH_BUILD_ID"), 16) { Ok(id) => id, Err(_) => 0 };

/// What names a program: a hash of its source text and every function's registers and opcodes
/// (atom numbers are per VM, so no operand), which an image is valid for and no other (plus the helper count the code was linked against; it is a validity key, never a reason to treat a program specially).
fn fingerprint(program: &ResidualProgram) -> u64 {
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

/// The image of `function`, or `None` where it lowers to nothing a lane body may run.
fn image(program: &ResidualProgram, function: u32) -> Option<Image> {
    let body = program.function_get(function as usize)?;
    let unit = lane_unit::with_budget(program, function, body, 0, &|_, _| Vec::new(), &|callee| program.function_get(callee as usize))?;
    let mut lowered = lane_region::lower_body(program, function, &unit, LaneHelpers::Ahead, &BTreeMap::new(),
        &Default::default(), VmFacts::STATIC, &[], &BTreeMap::new(), &[], None, &[])?;
    if lowered.region.native == 0 { return None; }
    let native = std::mem::replace(&mut lowered.region.function, ir::Function::new());
    let isa = crate::vm::jit_backend::target_isa()?;
    let code = crate::vm::jit_backend::compile_function(native, &*isa, Some(lane_region::LANE_BODY_MAX_CODE_BYTES), |compiled, function| {
        RelocatableCode::extract(compiled, function).ok_or(NativeCompileRejection::Relocations)
    }).ok()?;
    let atoms = code.relocations.iter().filter_map(|relocation| lane_region::relocs::atom_key(relocation.namespace, relocation.key))
        .collect::<std::collections::BTreeSet<_>>().into_iter().map(|atom| (atom, program.atoms[atom as usize].to_string())).collect();
    Some(Image { fault_poll: lowered.region.fault_poll, code, atoms })
}

impl AheadImages {
    /// The images of every function of `program` that has one.
    pub fn compile(program: &ResidualProgram) -> Self {
        let images = (1..program.function_count()).filter_map(|function| Some((function as u32, image(program, function as u32)?))).collect();
        Self { program: fingerprint(program), known: Default::default(), images }
    }

    /// The image of `function`.
    fn get(&self, function: u32) -> Option<&Image> {
        self.images.binary_search_by_key(&function, |(of, _)| *of).ok().map(|at| &self.images[at].1)
    }

    /// Whether these are the images of `program`, the program numbered `id`; each program is
    /// fingerprinted once.
    pub(in crate::vm) fn is_of(&self, id: u32, program: &ResidualProgram) -> bool {
        let mut known = self.known.borrow_mut();
        if let Some((_, of)) = known.iter().find(|(known, _)| *known == id) { return *of; }
        let of = fingerprint(program) == self.program;
        known.push((id, of));
        of
    }

    /// How many functions have an image.
    pub fn len(&self) -> usize { self.images.len() }

    /// Whether no function has an image.
    pub fn is_empty(&self) -> bool { self.images.is_empty() }

    /// The file of these images: a magic, the build id, then per image its function, entry facts, code and
    /// relocations (offset, namespace, key, addend), every number little-endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend(BUILD_ID.to_le_bytes());
        out.extend(self.program.to_le_bytes());
        out.extend((self.images.len() as u32).to_le_bytes());
        for (function, image) in &self.images {
            out.extend(function.to_le_bytes());
            out.push(u8::from(image.fault_poll));
            out.extend((image.code.code.len() as u32).to_le_bytes());
            out.extend(&*image.code.code);
            out.extend((image.code.relocations.len() as u32).to_le_bytes());
            for relocation in &*image.code.relocations {
                out.extend((relocation.offset as u32).to_le_bytes());
                out.extend(relocation.namespace.to_le_bytes());
                out.extend(relocation.key.to_le_bytes());
                out.extend(relocation.addend.to_le_bytes());
            }
            out.extend((image.atoms.len() as u32).to_le_bytes());
            for (atom, text) in &image.atoms {
                out.extend(atom.to_le_bytes());
                out.extend((text.len() as u32).to_le_bytes());
                out.extend(text.as_bytes());
            }
        }
        out
    }

    /// The images `to_bytes` wrote; an error names what the file lacks.
    pub fn from_bytes(file: &[u8]) -> Result<Self, String> {
        let mut words = Words(file.strip_prefix(MAGIC.as_slice()).ok_or("not an ahead image file")?);
        if words.quad()? != BUILD_ID { return Err("images from another quench-node build".into()); }
        let program = words.quad()?;
        let mut images = Vec::new();
        for _ in 0..words.half()? {
            let (function, fault_poll) = (words.half()?, words.byte()? != 0);
            let length = words.half()? as usize;
            let code: Box<[u8]> = words.bytes(length)?.into();
            let relocations = (0..words.half()?).map(|_| Ok(Relocation {
                offset: words.half()? as usize, kind: cranelift_codegen::binemit::Reloc::Abs8,
                namespace: words.half()?, key: words.half()?, addend: words.quad()? as i64 })).collect::<Result<_, String>>()?;
            let atoms = (0..words.half()?).map(|_| {
                let atom = words.half()?;
                let length = words.half()? as usize;
                Ok((atom, String::from_utf8(words.bytes(length)?.to_vec()).map_err(|_| "an atom is not text")?))
            }).collect::<Result<_, String>>()?;
            images.push((function, Image { fault_poll, code: RelocatableCode { code, relocations }, atoms }));
        }
        images.sort_by_key(|(function, _)| *function);
        Ok(Self { program, known: Default::default(), images })
    }
}

/// The images of each program stored while recording (`start_recording`); the entry may run
/// on a worker thread, so this is process-wide.
static RECORDED: std::sync::Mutex<Option<Vec<AheadImages>>> = std::sync::Mutex::new(None);

fn recorded() -> std::sync::MutexGuard<'static, Option<Vec<AheadImages>>> { RECORDED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) }

impl AheadImages {
    /// Starts collecting the images of every program this thread stores (`finish_recording`).
    pub fn start_recording() { *recorded() = Some(Vec::new()); }

    /// Adds `program`'s images while recording; a program with none adds none.
    pub(in crate::vm) fn record(program: &ResidualProgram) {
        if recorded().is_none() { return; }
        let images = Self::compile(program);
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

/// A reader of the file's numbers.
struct Words<'a>(&'a [u8]);

impl<'a> Words<'a> {
    fn bytes(&mut self, count: usize) -> Result<&'a [u8], String> {
        let (head, rest) = self.0.split_at_checked(count).ok_or("an ahead image file ends early")?;
        self.0 = rest;
        Ok(head)
    }
    fn take<const N: usize>(&mut self) -> Result<[u8; N], String> { Ok(self.bytes(N)?.try_into().expect("N bytes")) }
    fn byte(&mut self) -> Result<u8, String> { Ok(u8::from_le_bytes(self.take()?)) }
    fn half(&mut self) -> Result<u32, String> { Ok(u32::from_le_bytes(self.take()?)) }
    fn quad(&mut self) -> Result<u64, String> { Ok(u64::from_le_bytes(self.take()?)) }
}

impl<H: Host> Vm<H> {
    /// `function`'s image linked to this VM's helpers and published as its lane body, if the
    /// loaded images hold one for this program.
    pub(in crate::vm) fn install_ahead_body(&mut self, program: &ResidualProgram, function: u32, entry: Option<u32>, generic: &[LaneExit]) -> Option<Rc<PublishedLaneMethod>> {
        if entry.is_some() || !generic.is_empty() { return None; }
        let images = self.programs.ahead_images(self.active_program, program)?;
        let image = images.get(function)?;
        let body = program.function_get(function as usize)?;
        let unit = lane_unit::with_budget(program, function, body, 0, &|_, _| Vec::new(), &|callee| program.function_get(callee as usize))?;
        let table = LaneHelper::addresses::<H>();
        let atoms: Vec<(u32, u32)> = image.atoms.iter().map(|(atom, text)| (*atom, self.intern_atom(text))).collect();
        let linked = image.code.link(0, |namespace, key| match lane_region::relocs::atom_key(namespace, key) {
            Some(atom) => atoms.iter().find(|(named, _)| *named == atom).map(|(_, here)| u64::from(*here)),
            None => table.get(lane_region::relocs::helper_key(namespace, key)?).map(|address| *address as usize as u64),
        }).ok()?;
        let region = lane_region::LoweredRegion { number_result: None, function: ir::Function::new(), native: 1,
            fault_poll: image.fault_poll, hoists_arrays: false, stable: Default::default() };
        let window_frames = lane_unit::frames_above_window(program, function, &unit);
        let plan = lane_compile::LanePlan { unit, lowered: lane_region::LoweredLane { region, clif: None }, fields: BTreeMap::new(),
            direct: Box::new([]), closed: false, function, window_frames, invalidation: crate::code_dependencies::Invalidation::default(), budget: 0 };
        let method = plan.finish(&linked)?;
        self.programs.publish_lane_method(method, &mut self.heap, self.method_cache_atoms.readers())
    }

    /// Takes `images`; they serve the program they were compiled from, whenever it runs.
    pub fn load_ahead_images(&mut self, images: AheadImages) {
        self.programs.add_ahead_images(images);
    }
}
