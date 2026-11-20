//! The live linker: an `AotImage` placed in this process's executable memory. Every base and lane
//! body sits in one code block whose layout fixes every address before any code is linked, so a
//! base PC leave resolves to its final address. Every other symbol resolves now: helpers,
//! semantic entries, atoms, the invalidation word, heap values and shapes (`symbols::resolve_recipe`).
//! A function's entry is its lane body where it has one, else its base body.
//!
//! Owner: one pipeline.

use super::{AotImage, base::NO_ENTRY, symbols::{self, Symbol, SymbolTable}};
use crate::code_dependencies::Invalidation;
use crate::vm::Vm;
use crate::vm::compiled::{CodeMemory, LaneHelper};
use crate::vm::jit_backend::relocatable_code::RelocatableCode;
use crate::Host;

/// Every body starts at this alignment in the block.
const CODE_ALIGNMENT: usize = 16;

/// An image linked into this process: its code placed and every symbol resolved.
pub(crate) struct LiveImage {
    /// Every base and lane body, placed once; the addresses below point into it.
    _code: CodeMemory,
    /// The word the code's invalidation points read; it lives as long as the code.
    _invalidation: Invalidation,
    /// Each function's entry: its lane body where it has one, else its base body.
    pub(crate) entries: Box<[usize]>,
    /// Each function's base body.
    pub(crate) bases: Box<[usize]>,
    /// Each function's lane body, where it has one.
    pub(crate) lanes: Box<[Option<usize>]>,
}

/// Where each body sits in the block (offsets), and the block's length.
struct Layout {
    bases: Box<[usize]>,
    lanes: Box<[Option<usize>]>,
    length: usize,
}

/// Links `image` into `vm`: its code placed, every symbol resolved.
pub(crate) fn install<H: Host>(image: &AotImage, vm: &mut Vm<H>) -> Result<LiveImage, String> {
    let layout = layout(image);
    let invalidation = Invalidation::default();
    let word = invalidation.address();
    let code = CodeMemory::assemble(layout.length, |pointer, out| place(image, vm, &layout, pointer.addr(), word, out))?;
    let start = code.pointer().addr();
    let bases: Box<[usize]> = layout.bases.iter().map(|at| start + at).collect();
    let lanes: Box<[Option<usize>]> = layout.lanes.iter().map(|lane| lane.map(|at| start + at)).collect();
    let entries = bases.iter().zip(lanes.iter()).map(|(base, lane)| lane.unwrap_or(*base)).collect();
    Ok(LiveImage { _code: code, _invalidation: invalidation, entries, bases, lanes })
}

/// The lane body of `function` linked against `live`, at address 0 for a copy into a new body
/// (`LanePlan::finish`); `None` where the function has no lane body or a symbol does not link.
pub(crate) fn link_lane<H: Host>(image: &AotImage, vm: &mut Vm<H>, live: &LiveImage, function: u32) -> Option<Vec<u8>> {
    let lane = image.functions.get(function as usize)?.lane.as_ref()?;
    let word = live._invalidation.address();
    let bases: Box<[u64]> = live.bases.iter().map(|base| *base as u64).collect();
    lane.code.link(0, |namespace, key| resolve(image, vm, &bases, word, namespace, key)).ok()
}

/// The offsets of every body, each aligned, and the block's length.
fn layout(image: &AotImage) -> Layout {
    let mut length = 0;
    let mut reserve = |code: &RelocatableCode| {
        let at = length;
        length = (at + code.code.len()).next_multiple_of(CODE_ALIGNMENT);
        at
    };
    let bases = image.functions.iter().map(|function| reserve(&function.base.code)).collect();
    let lanes = image.functions.iter().map(|function| function.lane.as_ref().map(|lane| reserve(&lane.code))).collect();
    Layout { bases, lanes, length }
}

/// Links every body of `image` into `out`, the block that starts at `start` with `invalidation`.
fn place<H: Host>(image: &AotImage, vm: &mut Vm<H>, layout: &Layout, start: usize, invalidation: usize, out: &mut [u8]) -> Result<(), String> {
    let bases: Box<[u64]> = layout.bases.iter().map(|at| (start + at) as u64).collect();
    let mut address = |namespace: u32, key: u32| resolve(image, vm, &bases, invalidation, namespace, key);
    for (function, body) in image.functions.iter().enumerate() {
        link_into(out, start, layout.bases[function], &body.base.code, &mut address)?;
        if let (Some(lane), Some(at)) = (&body.lane, layout.lanes[function]) {
            link_into(out, start, at, &lane.code, &mut address)?;
        }
    }
    Ok(())
}

/// Links `code` to run at `start + at` and copies it into `out` at `at`.
fn link_into(out: &mut [u8], start: usize, at: usize, code: &RelocatableCode, resolve: &mut impl FnMut(u32, u32) -> Option<u64>) -> Result<(), String> {
    let linked = code.link((start + at) as u64, &mut *resolve)?;
    out.get_mut(at..at + linked.len()).ok_or("AOT image block is too small")?.copy_from_slice(&linked);
    Ok(())
}

/// The address of symbol `(namespace, key)` in this process, or `None` where it resolves to none yet.
fn resolve<H: Host>(image: &AotImage, vm: &mut Vm<H>, bases: &[u64], invalidation: usize, namespace: u32, key: u32) -> Option<u64> {
    let symbol = SymbolTable::decode(&image.labels, namespace, key)?;
    match symbol {
        Symbol::Helper(helper) => LaneHelper::addresses::<H>().get(helper as usize).map(|address| *address as usize as u64),
        Symbol::SemanticEntry(op) => semantic_entry::<H>(op),
        Symbol::Atom(atom) => Some(u64::from(vm.intern_atom(image.atoms.get(atom as usize)?))),
        Symbol::Invalidation => Some(invalidation as u64),
        Symbol::BasePc { function, pc } => base_pc(image, bases, function, pc),
        Symbol::HeapValue(_) | Symbol::Shape(_) => symbols::resolve_recipe(symbol, image, vm),
        Symbol::EntryCell(_) | Symbol::Descriptor(_) | Symbol::CaptureWords(_) | Symbol::ConstantPool(_) => None,
    }
}

/// The address of opcode `op`'s monomorphized semantic entry.
#[cfg(feature = "native-execution")]
fn semantic_entry<H: Host>(op: u16) -> Option<u64> {
    crate::vm::compiled::native_js_operation_entry_for::<H>(op as usize).map(|entry| entry as usize as u64)
}

#[cfg(not(feature = "native-execution"))]
fn semantic_entry<H: Host>(_op: u16) -> Option<u64> {
    None
}

/// The address of record `pc` of function `function`'s base body, if control enters it.
fn base_pc(image: &AotImage, bases: &[u64], function: u32, pc: u32) -> Option<u64> {
    let offset = *image.functions.get(function as usize)?.base.pc_offsets.get(pc as usize)?;
    if offset == NO_ENTRY { return None; }
    bases.get(function as usize).map(|base| base + u64::from(offset))
}
