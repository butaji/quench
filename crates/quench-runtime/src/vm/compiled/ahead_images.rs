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
}

/// A program's ahead-of-time lane bodies by function.
#[derive(Debug, Default)]
pub struct AheadImages {
    /// By function, ascending.
    images: Vec<(u32, Image)>,
}

const MAGIC: &[u8; 8] = b"QAHEAD\0\x01";

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
    Some(Image { fault_poll: lowered.region.fault_poll, code })
}

impl AheadImages {
    /// The images of every function of `program` that has one.
    pub fn compile(program: &ResidualProgram) -> Self {
        let images = (1..program.function_count()).filter_map(|function| Some((function as u32, image(program, function as u32)?))).collect();
        Self { images }
    }

    /// The image of `function`.
    fn get(&self, function: u32) -> Option<&Image> {
        self.images.binary_search_by_key(&function, |(of, _)| *of).ok().map(|at| &self.images[at].1)
    }

    /// How many functions have an image.
    pub fn len(&self) -> usize { self.images.len() }

    /// Whether no function has an image.
    pub fn is_empty(&self) -> bool { self.images.is_empty() }

    /// The file of these images: a magic, then per image its function, entry facts, code and
    /// relocations (offset, namespace, key, addend), every number little-endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
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
        }
        out
    }

    /// The images `to_bytes` wrote; an error names what the file lacks.
    pub fn from_bytes(file: &[u8]) -> Result<Self, String> {
        let mut words = Words(file.strip_prefix(MAGIC.as_slice()).ok_or("not an ahead image file")?);
        let mut images = Vec::new();
        for _ in 0..words.half()? {
            let (function, fault_poll) = (words.half()?, words.byte()? != 0);
            let length = words.half()? as usize;
            let code: Box<[u8]> = words.bytes(length)?.into();
            let relocations = (0..words.half()?).map(|_| Ok(Relocation {
                offset: words.half()? as usize, kind: cranelift_codegen::binemit::Reloc::Abs8,
                namespace: words.half()?, key: words.half()?, addend: words.quad()? as i64 })).collect::<Result<_, String>>()?;
            images.push((function, Image { fault_poll, code: RelocatableCode { code, relocations } }));
        }
        images.sort_by_key(|(function, _)| *function);
        Ok(Self { images })
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
        let images = self.programs.ahead_images(self.active_program)?;
        let image = images.get(function)?;
        let body = program.function_get(function as usize)?;
        let unit = lane_unit::with_budget(program, function, body, 0, &|_, _| Vec::new(), &|callee| program.function_get(callee as usize))?;
        let table = LaneHelper::addresses::<H>();
        let linked = image.code.link(0, |namespace, key| table.get(lane_region::relocs::helper_key(namespace, key)?).map(|address| *address as usize as u64)).ok()?;
        let region = lane_region::LoweredRegion { number_result: None, function: ir::Function::new(), native: 1,
            fault_poll: image.fault_poll, hoists_arrays: false, stable: Default::default() };
        let plan = lane_compile::LanePlan { unit, lowered: lane_region::LoweredLane { region, clif: None }, fields: BTreeMap::new(),
            direct: Box::new([]), closed: false, function, invalidation: crate::code_dependencies::Invalidation::default(), budget: 0 };
        let method = plan.finish(&linked)?;
        self.programs.publish_lane_method(method, &mut self.heap, self.method_cache_atoms.readers())
    }

    /// Takes `images` as `program`'s ahead-of-time bodies.
    pub fn load_ahead_images(&mut self, images: AheadImages) {
        self.programs.set_ahead_images(self.active_program, images);
    }
}
