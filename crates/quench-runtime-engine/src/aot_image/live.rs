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
/// the invalidation word, the call records and the linked helpers (`LiveImage::helpers`).
struct Places<'a> {
    bases: &'a [u64],
    helpers: Option<&'a [usize]>,
    invalidation: usize,
    calls: &'a [UnsafeCell<CallRecord>],
}

/// Places the native searches of `image`'s RegExp literals (`AotImage::regexps`) and registers them on this
/// thread, so each literal's matcher runs its code from its first search (`quench_regexp::backtrack::native::register_ahead`).
/// An executable has no code memory to place them in without the `jit` feature: it links them (`linked_regexps`).
#[cfg(not(feature = "jit"))]
fn register_regexps(_: &AotImage) {}
#[cfg(feature = "jit")]
fn register_regexps(image: &AotImage) {
    for regexp in &image.regexps {
        if let Some((entry, keep)) = crate::regexp_native::place_ahead(&regexp.code) {
            quench_regexp::backtrack::native::register_ahead(regexp.key.clone(), entry, keep);
        }
    }
}

/// Registers the searches the system linker placed for `image`'s RegExp literals (`object::regexp_data`), in image order.
fn linked_regexps(image: &AotImage, table: &LinkedTable) {
    for (regexp, &address) in image.regexps.iter().zip(table.regexps).filter(|(_, address)| **address != 0) {
        // SAFETY: the linker bound the address to the literal's search, whose signature is `NativeFn` (`regexp_native::ahead_code`).
        let entry = unsafe { std::mem::transmute::<usize, quench_regexp::backtrack::native::NativeFn>(address) };
        quench_regexp::backtrack::native::register_ahead(regexp.key.clone(), entry, std::rc::Rc::new(()));
    }
}

/// Links `image` into `vm`: its code placed, every symbol resolved.
pub(crate) fn install(image: &AotImage, vm: &mut Vm) -> Result<LiveImage, String> {
    register_regexps(image);
    let layout = layout(image);
    let invalidation = Invalidation::default();
    let calls = BoundCalls::of(image);
    let mut linked = vec![false; image.functions.len()];
    let code = CodeMemory::assemble(layout.length, |pointer, out| {
        let bases: Box<[u64]> = layout.bases.iter().map(|at| (pointer.addr() + at) as u64).collect();
        let places = Places { bases: &bases, helpers: None, invalidation: invalidation.address(), calls: calls.records() };
        place(image, vm, &layout, (pointer.addr(), &places), (out, linked.as_mut_slice()))
    })?;
    let start = code.pointer().addr();
    let bases: Box<[usize]> = layout.bases.iter().map(|at| start + at).collect();
    let lanes: Box<[Option<usize>]> = layout.lanes.iter().zip(&linked)
        .map(|(lane, linked)| lane.filter(|_| *linked).map(|at| start + at)).collect();
    let entries = bases.iter().zip(lanes.iter()).map(|(base, lane)| lane.unwrap_or(*base)).collect();
    Ok(LiveImage { _code: Some(code), invalidation, calls, entries, bases, lanes, helpers: None })
}

pub(crate) use quench_runtime::vm::engine_types::LinkedTable;

#[quench_macros::ext(LinkedTableOps1)]
impl LinkedTable {
    /// The table at `table` of an image of `count` functions (`object::entry_table`): the ABI
    /// header, `count` entries, `count` base bodies, then the file's address and length, the call
    /// records' address, the linked helpers' address and count, and the RegExp searches' address and count.
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
            let helpers = std::slice::from_raw_parts(tail.add(3).read() as *const usize, tail.add(4).read());
            let regexps = std::slice::from_raw_parts(tail.add(5).read() as *const usize, tail.add(6).read());
            Ok(Self { entries, bases, file, calls: tail.add(2).read() as *mut CallRecord, helpers, regexps })
        }
    }
}

/// `image` as the system linker placed it (`table`, the entry table it was read from): no code is
/// placed or linked; a lane body a VM publishes is relinked from the image against the placed base
/// bodies and records (`link_lane`).
pub(crate) fn linked(image: &AotImage, table: &LinkedTable) -> Result<LiveImage, String> {
    linked_regexps(image, table);
    let count = image.functions.len();
    if table.entries.len() != count || table.bases.len() != count {
        return Err("native image table does not match its image".into());
    }
    // SAFETY: the table names the image's writable call records, alive for the process (`LinkedTable::read`).
    let calls = unsafe { BoundCalls::linked(image, table.calls) };
    // The object places no lane body (`object::codes`): each is relinked from the image when a VM installs it.
    let lanes = vec![None; count].into_boxed_slice();
    Ok(LiveImage { _code: None, invalidation: Invalidation::default(), calls,
        entries: table.entries.into(), bases: table.bases.into(), lanes, helpers: Some(table.helpers) })
}

/// The lane body of `function` linked against `live`, at address 0 for a copy into a new body
/// (`LanePlan::finish`), with the heap values it names (`HeapValueRecipe::Global`, `PrototypeOf`, `ClosureOf`),
/// which its publication keeps alive (`PublishedCode::hold_roots`): a collected object's id is
/// reused, and the code would take the next object for it. `None` where the function has no lane
/// body or a symbol does not link.
pub(crate) fn link_lane(image: &AotImage, vm: &mut Vm, live: &LiveImage, function: u32) -> Option<(Vec<u8>, FoldedRoots)> {
    let lane = image.functions.get(function as usize)?.lane.as_ref()?;
    let bases: Box<[u64]> = live.bases.iter().map(|base| *base as u64).collect();
    let places = Places { bases: &bases, helpers: live.helpers, invalidation: live.invalidation.address(), calls: live.calls.records() };
    let mut roots = FoldedRoots::default();
    let code = lane.code.link(0, |namespace, key| {
        let word = resolve(image, vm, &places, namespace, key)?;
        if let Some(Symbol::HeapValue(_)) = SymbolTable::decode(image, namespace, key) { roots.embed_heap_identity(quench_runtime::Value::from_bits(word)); }
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
fn resolve(image: &AotImage, vm: &mut Vm, places: &Places<'_>, namespace: u32, key: u32) -> Option<u64> {
    let symbol = SymbolTable::decode(image, namespace, key)?;
    match symbol {
        // The linked image's helpers are the ones its object names, so the linker keeps no other; an image placed in
        // process takes the process's table, which only a build with the JIT holds (`quench compile`, a `.qimg`).
        Symbol::Helper(helper) => match places.helpers {
            Some(linked) => linked.get(helper as usize).copied().filter(|address| *address != 0).map(|address| address as u64),
            None if cfg!(feature = "jit") => LaneHelper::addresses().get(helper as usize).map(|address| *address as usize as u64),
            None => None,
        },
        Symbol::CallRecord { .. } => places.calls.get(key as usize).map(|record| record.get().addr() as u64),
        Symbol::Atom(atom) => Some(u64::from(vm.intern_atom(image.atoms.get(atom as usize)?))),
        Symbol::Invalidation => Some(places.invalidation as u64),
        Symbol::NarrowUpperWords => Some(quench_runtime::value::NarrowLayout::upper_words() as u64),
        Symbol::Attention => Some(quench_runtime::execution_interrupt::attention_address()),
        Symbol::BasePc { function, pc } => base_pc(image, places.bases, function, pc),
        Symbol::HeapValue(_) | Symbol::Shape(_) => symbols::resolve_recipe(symbol, image, vm),
        Symbol::EntryCell(_) | Symbol::Descriptor(_) | Symbol::CaptureWords(_) | Symbol::ConstantPool(_) => None,
    }
}

/// The address of record `pc` of function `function`'s base body, if control enters it.
fn base_pc(image: &AotImage, bases: &[u64], function: u32, pc: u32) -> Option<u64> {
    let offset = *image.functions.get(function as usize)?.base.pc_offsets.get(pc as usize)?;
    if offset == NO_ENTRY { return None; }
    bases.get(function as usize).map(|base| base + u64::from(offset))
}

