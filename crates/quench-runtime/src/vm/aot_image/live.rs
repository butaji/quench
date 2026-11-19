//! The live linker: an `AotImage` placed in this process's executable memory. Every base and lane
//! body sits in one code block whose layout fixes every address before any code is linked, so a
//! base PC leave resolves to its final address. Every other symbol resolves now: helpers,
//! semantic entries, operand records (the image's table, kept with the code), atoms, the
//! invalidation word, heap values and shapes (`symbols::resolve_recipe`), and the records of the
//! calls the closed world proves (the image's templates, kept with the code), which name no callee
//! until a publication binds each to the closure its site resolved (`bind_call`).
//! A function's entry is its lane body where it has one, else its base body.
//!
//! Owner: one pipeline.

use super::{AotImage, base::NO_ENTRY, symbols::{self, Symbol, SymbolTable}};
use crate::code_dependencies::Invalidation;
use crate::folded_roots::FoldedRoots;
use crate::vm::Vm;
use crate::vm::compiled::{CodeMemory, DirectCallee, LaneHelper, PublishedLaneMethod};
use quench_bjit::call_record::CallRecord;
use std::cell::UnsafeCell;
use crate::vm::jit_backend::relocatable_code::RelocatableCode;
use crate::Host;

/// Every body starts at this alignment in the block.
const CODE_ALIGNMENT: usize = 16;

/// An image linked into this process: its code placed and every symbol resolved.
pub(crate) struct LiveImage {
    /// Every base and lane body, placed once; the addresses below point into it.
    _code: CodeMemory,
    /// The image's one invalidation word, which every body's invalidation points read; it lives
    /// as long as the code, and every lane body published from the image retires through it
    /// (`Vm::install_ahead_body`), so a broken fact of any one stops every fold.
    pub(crate) invalidation: Invalidation,
    /// The operand records the generic rows pass their semantic entries; they live as long as the code.
    operands: Box<[[u64; 2]]>,
    /// The records the direct rows of proven calls read in place, by `Symbol::CallRecord` key; they
    /// live as long as the code.
    calls: Box<[UnsafeCell<CallRecord>]>,
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

/// The addresses symbols resolve to that the image's own data fixes: each function's base body,
/// the invalidation word, the operand table and the call records.
struct Places<'a> {
    bases: &'a [u64],
    invalidation: usize,
    operands: &'a [[u64; 2]],
    calls: &'a [UnsafeCell<CallRecord>],
}

/// Links `image` into `vm`: its code placed, every symbol resolved.
pub(crate) fn install<H: Host>(image: &AotImage, vm: &mut Vm<H>) -> Result<LiveImage, String> {
    let layout = layout(image);
    let invalidation = Invalidation::default();
    let operands: Box<[[u64; 2]]> = image.operands.iter().map(|record| record.words).collect();
    let calls: Box<[UnsafeCell<CallRecord>]> = image.calls.iter().map(|call| UnsafeCell::new(call.record)).collect();
    let mut linked = vec![false; image.functions.len()];
    let code = CodeMemory::assemble(layout.length, |pointer, out| {
        let bases: Box<[u64]> = layout.bases.iter().map(|at| (pointer.addr() + at) as u64).collect();
        let places = Places { bases: &bases, invalidation: invalidation.address(), operands: &operands, calls: &calls };
        place(image, vm, &layout, (pointer.addr(), &places), (out, linked.as_mut_slice()))
    })?;
    let start = code.pointer().addr();
    let bases: Box<[usize]> = layout.bases.iter().map(|at| start + at).collect();
    let lanes: Box<[Option<usize>]> = layout.lanes.iter().zip(&linked)
        .map(|(lane, linked)| lane.filter(|_| *linked).map(|at| start + at)).collect();
    let entries = bases.iter().zip(lanes.iter()).map(|(base, lane)| lane.unwrap_or(*base)).collect();
    Ok(LiveImage { _code: code, invalidation, operands, calls, entries, bases, lanes })
}

/// The lane body of `function` linked against `live`, at address 0 for a copy into a new body
/// (`LanePlan::finish`), with the heap values it names (`HeapValueRecipe::Global`, `PrototypeOf`),
/// which its publication keeps alive (`PublishedCode::hold_roots`): a collected object's id is
/// reused, and the code would take the next object for it. `None` where the function has no lane
/// body or a symbol does not link.
pub(crate) fn link_lane<H: Host>(image: &AotImage, vm: &mut Vm<H>, live: &LiveImage, function: u32) -> Option<(Vec<u8>, FoldedRoots)> {
    let lane = image.functions.get(function as usize)?.lane.as_ref()?;
    let bases: Box<[u64]> = live.bases.iter().map(|base| *base as u64).collect();
    let places = Places { bases: &bases, invalidation: live.invalidation.address(), operands: &live.operands, calls: &live.calls };
    let mut roots = FoldedRoots::default();
    let code = lane.code.link(0, |namespace, key| {
        let word = resolve(image, vm, &places, namespace, key)?;
        if let Some(Symbol::HeapValue(_)) = SymbolTable::decode(image, namespace, key) { roots.embed_heap_identity(crate::Value::from_bits(word)); }
        Some(word)
    }).ok()?;
    Some((code, roots.settled()))
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

/// Links every body of `image` into `out`, the block that starts at `start`, its symbols at
/// `places`, and marks in `linked` each lane body that linked. A base body names only symbols the
/// image fixes at load, so one that does not link fails the image; a lane body may name what the
/// program makes as it runs (`HeapValueRecipe::PrototypeOf`, a callee's entry cell), which
/// resolves when it is published (`link_lane`), so one that does not link yet is left unplaced.
fn place<H: Host>(image: &AotImage, vm: &mut Vm<H>, layout: &Layout, (start, places): (usize, &Places<'_>),
    (out, linked): (&mut [u8], &mut [bool])) -> Result<(), String> {
    let mut address = |namespace: u32, key: u32| resolve(image, vm, places, namespace, key);
    for (function, body) in image.functions.iter().enumerate() {
        link_into(out, start, layout.bases[function], &body.base.code, &mut address)?;
        if let (Some(lane), Some(at)) = (&body.lane, layout.lanes[function]) {
            linked[function] = link_into(out, start, at, &lane.code, &mut address).is_ok();
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
fn resolve<H: Host>(image: &AotImage, vm: &mut Vm<H>, places: &Places<'_>, namespace: u32, key: u32) -> Option<u64> {
    let symbol = SymbolTable::decode(image, namespace, key)?;
    match symbol {
        Symbol::Helper(helper) => LaneHelper::addresses::<H>().get(helper as usize).map(|address| *address as usize as u64),
        Symbol::SemanticEntry(op) => semantic_entry::<H>(op),
        Symbol::Operands { .. } => places.operands.get(key as usize).map(|record| record.as_ptr().addr() as u64),
        Symbol::CallRecord { .. } => places.calls.get(key as usize).map(|record| record.get().addr() as u64),
        Symbol::Atom(atom) => Some(u64::from(vm.intern_atom(image.atoms.get(atom as usize)?))),
        Symbol::Invalidation => Some(places.invalidation as u64),
        Symbol::BasePc { function, pc } => base_pc(image, places.bases, function, pc),
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

/// Binds the record of call site `key` to `direct`, the callee its site resolved, which `owner` owns:
/// the record's run-time words become the callee's, as a JIT plan's record holds them
/// (`DirectFacts::of`), its identity last, which admits the rows. The owner's retirement takes the
/// identity back to `unbound_callee` (`PublishedCode::on_retire`), so no row calls through a
/// descriptor the owner freed or a callee a collection may have freed with it.
pub(crate) fn bind_call(live: &LiveImage, key: usize, direct: &DirectCallee, owner: &PublishedLaneMethod) -> Option<()> {
    let record = live.calls.get(key)?.get();
    let identity = direct.callee.to_bits();
    let validity = (direct.cell + crate::staged::call::ENTRY_CELL.validity_offset as usize) as *const u64;
    // SAFETY: the record is the image's, read in place by code on this thread, none of which runs
    // during a publication; the cell is the callee's live entry cell (`DirectCallee::cell`), and the
    // callee word is a whole word of the record, which `on_retire` writes back as a `Cell`.
    unsafe {
        *record = CallRecord { callee: super::unbound_callee(), cell: direct.cell as u64, expected: validity.read(),
            descriptor: std::ptr::from_ref(direct).addr() as u64, captures: direct.capture_word(),
            global_this: direct.sloppy.map_or(0, crate::Value::to_bits), ..*record };
        (*record).callee = identity;
        let word = &*std::ptr::addr_of!((*record).callee).cast::<std::cell::Cell<usize>>();
        owner.on_retire(word, identity as usize, super::unbound_callee() as usize);
    }
    Some(())
}
