//! The live linker: an `AotImage` placed in this process's executable memory. Every base and lane
//! body sits in one code block whose layout fixes every address before any code is linked, so a
//! base PC leave resolves to its final address. Every other symbol resolves now: helpers,
//! semantic entries, operand records (the image's table, kept with the code), atoms, the
//! invalidation word, heap values and shapes (`symbols::resolve_recipe`), and the records of the
//! calls the closed world proves (`BoundCalls`, kept with the code).
//! A function's entry is its lane body where it has one, else its base body.
//!
//! Owner: one pipeline.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{AotImage, base::NO_ENTRY, symbols::{self, Symbol, SymbolTable}};
use quench_runtime::code_dependencies::Invalidation;
use quench_runtime::folded_roots::FoldedRoots;
use quench_runtime::vm::Vm;
use super::bound::BoundCalls;
use crate::compiled::{CodeMemory, LaneHelper};
use quench_bjit::call_record::CallRecord;
use std::cell::UnsafeCell;
use quench_runtime::vm::jit_backend::relocatable_code::RelocatableCode;

/// Every body starts at this alignment in the block.
const CODE_ALIGNMENT: usize = 16;

pub(crate) use quench_runtime::vm::engine_types::LiveImage;

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
    /// The address of the value table (`Symbol::Atom`, `HeapValue`, `Shape`): one 64-bit word per
    /// atom (its id), heap value (its bits) and shape (its id), placed beside the code so an
    /// `adrp` reaches it and a value relocation can load it.
    values: u64,
    /// How many words that table holds, per section.
    value_counts: (usize, usize, usize),
}

/// Links `image` into `vm`: its code placed, every symbol resolved.
pub(crate) fn install(image: &AotImage, vm: &mut Vm) -> Result<LiveImage, String> {
    let layout = layout(image);
    let invalidation = Invalidation::default();
    let operands: Box<[[u64; 2]]> = image.operands.iter().map(|record| record.words).collect();
    let calls = BoundCalls::of(image);
    let mut linked = vec![false; image.functions.len()];
    let value_bytes = value_words(image) * std::mem::size_of::<u64>();
    let code = CodeMemory::assemble(layout.length + value_bytes, |pointer, out| {
        let bases: Box<[u64]> = layout.bases.iter().map(|at| (pointer.addr() + at) as u64).collect();
        let values = pointer.addr() + layout.length;
        let unresolved = place_values(image, vm, out, layout.length)?;
        for function in &image.functions {
            let codes = std::iter::once(&function.base.code).chain(function.lane.as_ref().map(|lane| &lane.code));
            for code in codes { require_placed(code, &unresolved)?; }
        }
        let places = Places { bases: &bases, invalidation: invalidation.address(), operands: &operands, calls: calls.records(),
            values: values as u64, value_counts: (image.atoms.len(), image.heap_values.len(), image.shapes.len()) };
        place(image, vm, &layout, (pointer.addr(), &places), (out, linked.as_mut_slice()))
    })?;
    let start = code.pointer().addr();
    let bases: Box<[usize]> = layout.bases.iter().map(|at| start + at).collect();
    let lanes: Box<[Option<usize>]> = layout.lanes.iter().zip(&linked)
        .map(|(lane, linked)| lane.filter(|_| *linked).map(|at| start + at)).collect();
    let entries = bases.iter().zip(lanes.iter()).map(|(base, lane)| lane.unwrap_or(*base)).collect();
    Ok(LiveImage { _code: Some(code), invalidation, operands: operands.into_vec().into(), calls, entries, bases, lanes })
}

pub(crate) use quench_runtime::vm::engine_types::LinkedTable;

#[quench_macros::ext(LinkedTableOps1)]
impl LinkedTable {
    /// The table at `table` of an image of `count` functions (`object::entry_table`): the ABI
    /// header, `count` entries, `count` base bodies, then the file's address and length, the operand
    /// records' address and the call records' address.
    ///
    /// # Safety
    /// `table` is the aligned entry table symbol of an image of exactly `count` functions, emitted by
    /// this build's object linker, linked unmodified and alive for the process.
    pub(crate) unsafe fn read(table: *const u8, count: usize) -> Result<Self, String> {
        if table.is_null() || !table.addr().is_multiple_of(std::mem::align_of::<usize>()) {
            return Err("native image table is missing or misaligned".into());
        }
        let header = quench_runtime::NATIVE_JS_IMAGE_HEADER_WORDS;
        // SAFETY: the caller's contract supplies the whole table.
        unsafe {
            let words = table.cast::<usize>();
            let entries = std::slice::from_raw_parts(words.add(header), count);
            let bases = std::slice::from_raw_parts(words.add(header + count), count);
            let tail = words.add(header + 2 * count);
            let file = std::slice::from_raw_parts(tail.read() as *const u8, tail.add(1).read());
            Ok(Self { entries, bases, file, operands: tail.add(2).read() as *const [u64; 2], calls: tail.add(3).read() as *mut CallRecord })
        }
    }
}

/// `image` as the system linker placed it (`table`, the entry table it was read from): no code is
/// placed or linked; a lane body a VM publishes is relinked from the image against the placed base
/// bodies and records (`link_lane`).
pub(crate) fn linked(image: &AotImage, table: &LinkedTable) -> Result<LiveImage, String> {
    let count = image.functions.len();
    if table.entries.len() != count || table.bases.len() != count {
        return Err("native image table does not match its image".into());
    }
    // SAFETY: the table names the image's operand records, one per `image.operands`, alive for the
    // process, and its writable call records (`LinkedTable::read`).
    let (operands, calls) = unsafe { (std::slice::from_raw_parts(table.operands, image.operands.len()), BoundCalls::linked(image, table.calls)) };
    // The object places no lane body (`object::codes`): each is relinked from the image when a VM installs it.
    let lanes = vec![None; count].into_boxed_slice();
    Ok(LiveImage { _code: None, invalidation: Invalidation::default(), operands: std::borrow::Cow::Borrowed(operands), calls,
        entries: table.entries.into(), bases: table.bases.into(), lanes })
}

/// The lane body of `function` linked against `live`, at address 0 for a copy into a new body
/// (`LanePlan::finish`), with the heap values it names (`HeapValueRecipe::Global`, `PrototypeOf`, `ClosureOf`),
/// which its publication keeps alive (`PublishedCode::hold_roots`): a collected object's id is
/// reused, and the code would take the next object for it. `None` where the function has no lane
/// body or a symbol does not link.
pub(crate) fn link_lane(image: &AotImage, vm: &mut Vm, live: &LiveImage, function: u32) -> Option<(Vec<u8>, FoldedRoots)> {
    let lane = image.functions.get(function as usize)?.lane.as_ref()?;
    let bases: Box<[u64]> = live.bases.iter().map(|base| *base as u64).collect();
    let places = Places { bases: &bases, invalidation: live.invalidation.address(), operands: &live.operands, calls: live.calls.records(),
        values: 0, value_counts: (image.atoms.len(), image.heap_values.len(), image.shapes.len()) };
    let mut roots = FoldedRoots::default();
    let code = lane.code.link(0, |namespace, key| {
        let word = resolve(image, vm, &places, namespace, key)?;
        if let Some(Symbol::HeapValue(_)) = SymbolTable::decode(image, namespace, key) { roots.embed_heap_identity(quench_runtime::Value::from_bits(word)); }
        Some(word)
    }).ok()?;
    Some((code, roots.settled()))
}

/// Links `function`'s lane body at `address`, where it will run, writing its code and value table
/// to `out`: its `adrp`/`add` pairs and branches are page- and displacement-relative to that
/// address, so a body linked anywhere else faults or refuses to link. The returned roots are the
/// heap identities the code embeds, which its publication keeps alive.
pub(crate) fn link_lane_at(image: &AotImage, vm: &mut Vm, live: &LiveImage, function: u32, address: u64, out: &mut [u8]) -> Result<FoldedRoots, String> {
    let lane = image.functions.get(function as usize).and_then(|function| function.lane.as_ref())
        .ok_or_else(|| "no lane body".to_owned())?;
    let code_len = lane.code.code.len();
    let value_bytes = value_words(image) * std::mem::size_of::<u64>();
    if out.len() < code_len + value_bytes { return Err("lane body is larger than its placement".to_owned()) }
    // The value table follows the code in the same block, so its `adrp` is in range.
    let unresolved = place_values(image, vm, out, code_len)?;
    require_placed(&lane.code, &unresolved)?;
    let bases: Box<[u64]> = live.bases.iter().map(|base| *base as u64).collect();
    let places = Places { bases: &bases, invalidation: live.invalidation.address(), operands: &live.operands, calls: live.calls.records(),
        values: address + code_len as u64, value_counts: (image.atoms.len(), image.heap_values.len(), image.shapes.len()) };
    let mut roots = FoldedRoots::default();
    let mut symbol = None;
    let code = lane.code.link(address, |namespace, key| {
        symbol = SymbolTable::decode(image, namespace, key);
        let word = resolve(image, vm, &places, namespace, key)?;
        if let Some(Symbol::HeapValue(_)) = symbol { roots.embed_heap_identity(quench_runtime::Value::from_bits(word)); }
        Some(word)
    }).map_err(|error| format!("{error} [{symbol:?}]"))?;
    out[..code_len].copy_from_slice(&code);
    Ok(roots.settled())
}

/// The words the value table holds: one per atom, heap value and shape.
pub(crate) fn value_words(image: &AotImage) -> usize {
    image.atoms.len() + image.heap_values.len() + image.shapes.len()
}

/// The slot number of value `index` in `places`' table.
fn value_slot(places: &Places<'_>, index: u32) -> Option<u64> {
    let (atoms, heap, shapes) = places.value_counts;
    let index = index as usize;
    let bound = match index {
        _ if index < atoms => atoms,
        _ if index < atoms + heap => atoms + heap,
        _ => atoms + heap + shapes,
    };
    (index < bound).then(|| places.values + index as u64 * std::mem::size_of::<u64>() as u64)
}

/// Fills the value table at `at`: each atom's id, heap value's bits and shape's id, as this VM
/// resolved them, so a linking lowering's `adrp` loads the word it names.
fn place_values(image: &AotImage, vm: &mut Vm, out: &mut [u8], at: usize) -> Result<Vec<(u32, u32)>, String> {
    let size = std::mem::size_of::<u64>();
    let table = out.get_mut(at..at + value_words(image) * size).ok_or("AOT image block is too small for its values")?;
    let mut words = table.chunks_exact_mut(size);
    for text in &image.atoms {
        words.next().expect("the table holds every atom").copy_from_slice(&u64::from(vm.intern_atom(text)).to_le_bytes());
    }
    let mut unresolved = Vec::new();
    for key in 0..image.heap_values.len() as u32 {
        // A recipe the running program has not made yet (a closure it never made, a global it
        // never set) has no word: the body may still not name it (`link_lane_at` checks).
        let value = symbols::resolve_recipe(Symbol::HeapValue(key), image, vm).unwrap_or_else(|| {
            unresolved.push((super::super::compiled::RelocKind::HeapValue as u32, key));
            quench_runtime::Value::UNDEFINED.to_bits()
        });
        words.next().expect("the table holds every heap value").copy_from_slice(&value.to_le_bytes());
    }
    for key in 0..image.shapes.len() as u32 {
        let shape = symbols::resolve_recipe(Symbol::Shape(key), image, vm).unwrap_or_else(|| {
            unresolved.push((super::super::compiled::RelocKind::Shape as u32, key));
            0
        });
        words.next().expect("the table holds every shape").copy_from_slice(&shape.to_le_bytes());
    }
    Ok(unresolved)
}

/// Fails where `code` names a value `unresolved` (`(namespace, key)`) could not fill.
pub(crate) fn require_placed(code: &RelocatableCode, unresolved: &[(u32, u32)]) -> Result<(), String> {
    match unresolved.iter().copied().find(|&(namespace, key)| code.relocations.iter()
        .any(|relocation| relocation.namespace == namespace && relocation.key == key))
    {
        Some((namespace, key)) if namespace == super::super::compiled::RelocKind::Shape as u32 => {
            Err(format!("an AOT image shape {key} does not resolve"))
        }
        Some((_, key)) => Err(format!("an AOT image heap value {key} does not resolve")),
        None => Ok(()),
    }
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
fn place(image: &AotImage, vm: &mut Vm, layout: &Layout, (start, places): (usize, &Places<'_>),
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
fn resolve(image: &AotImage, _vm: &mut Vm, places: &Places<'_>, namespace: u32, key: u32) -> Option<u64> {
    let symbol = SymbolTable::decode(image, namespace, key)?;
    match symbol {
        Symbol::Helper(helper) => LaneHelper::addresses().get(helper as usize).map(|address| *address as usize as u64),
        Symbol::SemanticEntry(op) => semantic_entry(op),
        Symbol::Operands { .. } => places.operands.get(key as usize).map(|record| record.as_ptr().addr() as u64),
        Symbol::CallRecord { .. } => places.calls.get(key as usize).map(|record| record.get().addr() as u64),
        Symbol::Atom(atom) => value_slot(places, image.atoms.get(atom as usize).map(|_| u32::from(atom))?),
        Symbol::HeapValue(key) => value_slot(places, u32::from(key) + image.atoms.len() as u32),
        Symbol::Shape(key) => value_slot(places, u32::from(key) + (image.atoms.len() + image.heap_values.len()) as u32),
        Symbol::Invalidation => Some(places.invalidation as u64),
        Symbol::NarrowUpperWords => Some(quench_runtime::value::NarrowLayout::upper_words() as u64),
        Symbol::Attention => Some(quench_runtime::execution_interrupt::attention_address()),
        Symbol::BasePc { function, pc } => base_pc(image, places.bases, function, pc),
        Symbol::EntryCell(_) | Symbol::Descriptor(_) | Symbol::CaptureWords(_) | Symbol::ConstantPool(_) => None,
    }
}

/// The address of opcode `op`'s monomorphized semantic entry.
#[cfg(feature = "native-execution")]
fn semantic_entry(op: u16) -> Option<u64> {
    crate::compiled::native_js_operation_entry_for(op as usize).map(|entry| entry as usize as u64)
}

#[cfg(not(feature = "native-execution"))]
fn semantic_entry(_op: u16) -> Option<u64> {
    None
}

/// The address of record `pc` of function `function`'s base body, if control enters it.
fn base_pc(image: &AotImage, bases: &[u64], function: u32, pc: u32) -> Option<u64> {
    let offset = *image.functions.get(function as usize)?.base.pc_offsets.get(pc as usize)?;
    if offset == NO_ENTRY { return None; }
    bases.get(function as usize).map(|base| base + u64::from(offset))
}

