//! The live linker: an `AotImage` placed in this process's executable memory. Every base and lane
//! body sits in one code block whose layout fixes every address before any code is linked, so a
//! base PC leave resolves to its final address. Every other symbol resolves now: helpers, the
//! image's value table (each atom's id, heap value's bits and shape's id, placed beside the code
//! so an `adrp` reaches it), the invalidation word, and the records of the calls the closed world
//! proves (`BoundCalls`, kept with the code).
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

/// The bytes of one local veneer: `ldr x16, #8; br x16; .quad target`.
const VENEER_BYTES: usize = 16;
/// `ldr x16, #8`: the literal eight bytes on, the target.
const VENEER_LDR_X16: u32 = 0x5800_0050;
/// `br x16`.
const VENEER_BR_X16: u32 = 0xD61F_0200;

/// The stubs an `Arm64Call` the branch cannot reach takes: `ldr x16, #8; br x16; .quad target`,
/// written beside the code in the same block, which the branch reaches however far the block is
/// from the target (a code heap mmap'd ~885 MB from the executable's helpers on macOS arm64).
struct Veneers<'a> {
    base: u64,
    next: usize,
    out: &'a mut [u8],
}

impl Veneers<'_> {
    /// The address of a stub branching to `target`, written on first use.
    fn emit(&mut self, target: u64) -> u64 {
        let at = self.next * VENEER_BYTES;
        self.next += 1;
        let words = [VENEER_LDR_X16, VENEER_BR_X16, target as u32, (target >> 32) as u32];
        for (index, word) in words.iter().enumerate() {
            let word = word.to_le_bytes();
            self.out[at + index * 4..at + index * 4 + 4].copy_from_slice(&word);
        }
        self.base + at as u64
    }
}

/// The stub space one lane body's placement may need (`link_lane_at`).
pub(crate) fn stub_bytes_in_lane(code: &RelocatableCode) -> usize {
    code.relocations.iter().filter(|relocation| relocation.kind == cranelift_codegen::binemit::Reloc::Arm64Call).count() * VENEER_BYTES
}

/// The stub space a block of `image` may need: one per `Arm64Call`, the costliest case.
fn veneer_bytes(image: &AotImage) -> usize {
    image.functions.iter().map(|function| {
        std::iter::once(&function.base.code).chain(function.lane.as_ref().map(|lane| &lane.code))
            .map(|code| code.relocations.iter().filter(|relocation| relocation.kind == cranelift_codegen::binemit::Reloc::Arm64Call).count())
            .sum::<usize>()
    }).sum::<usize>() * VENEER_BYTES
}

pub(crate) use quench_runtime::vm::engine_types::LiveImage;

/// Where each body sits in the block (offsets), and the block's length.
struct Layout {
    bases: Box<[usize]>,
    lanes: Box<[Option<usize>]>,
    length: usize,
}

/// The addresses symbols resolve to that the image's own data fixes: each function's base body,
/// the invalidation word, the value table, the call records and the linked helpers
/// (`LiveImage::helpers`).
struct Places<'a> {
    bases: &'a [u64],
    helpers: Option<&'a [usize]>,
    invalidation: usize,
    calls: &'a [UnsafeCell<CallRecord>],
    /// The address of the value table (`Symbol::Atom`, `HeapValue`, `Shape`): one 64-bit word per
    /// atom (its id), heap value (its bits) and shape (its id), placed beside the code so an
    /// `adrp` reaches it and a value relocation can load it.
    values: u64,
    /// How many words that table holds, per section.
    value_counts: (usize, usize, usize),
}

/// Registers the image's RegExp searches: placed from the image where the JIT links it, else
/// linked into the executable (`linked_regexps`).
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

/// Links `image` into `vm`: its code and value table placed, every symbol resolved.
pub(crate) fn install(image: &AotImage, vm: &mut Vm) -> Result<LiveImage, String> {
    register_regexps(image);
    let layout = layout(image);
    let invalidation = Invalidation::default();
    let calls = BoundCalls::of(image);
    let mut linked = vec![false; image.functions.len()];
    let value_bytes = value_words(image) * std::mem::size_of::<u64>();
    let code = CodeMemory::assemble(layout.length + value_bytes + veneer_bytes(image), |pointer, out| {
        let (code_out, veneer_out) = out.split_at_mut(layout.length + value_bytes);
        let bases: Box<[u64]> = layout.bases.iter().map(|at| (pointer.addr() + at) as u64).collect();
        let values = pointer.addr() + layout.length;
        let unresolved = place_values(image, vm, code_out, layout.length)?;
        // A base body names only what the image itself fixes; a lane body may name a heap value the
        // program has not made yet, which `place` leaves unplaced until it is published.
        for function in &image.functions { require_placed(&function.base.code, &unresolved)?; }
        let places = Places { bases: &bases, helpers: None, invalidation: invalidation.address(), calls: calls.records(),
            values: values as u64, value_counts: (image.atoms.len(), image.heap_values.len(), image.shapes.len()) };
        let mut veneers = Veneers { base: values as u64 + value_bytes as u64, next: 0, out: veneer_out };
        place(image, vm, &layout, (pointer.addr(), &places), (code_out, linked.as_mut_slice()), &unresolved, &mut veneers)
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

/// Links `function`'s lane body at `address`, where it will run, writing its code and value table
/// to `out`: its `adrp`/`add` pairs and branches are page- and displacement-relative to that
/// address, so a body linked anywhere else faults or refuses to link. The returned roots are the
/// heap identities the code embeds, which its publication keeps alive.
pub(crate) fn link_lane_at(image: &AotImage, vm: &mut Vm, live: &LiveImage, function: u32, address: u64, out: &mut [u8]) -> Result<FoldedRoots, String> {
    let lane = image.functions.get(function as usize).and_then(|function| function.lane.as_ref())
        .ok_or_else(|| "no lane body".to_owned())?;
    let code_len = lane.code.code.len();
    let value_bytes = value_words(image) * std::mem::size_of::<u64>();
    let stubs = lane.code.relocations.iter().filter(|relocation| relocation.kind == cranelift_codegen::binemit::Reloc::Arm64Call).count() * VENEER_BYTES;
    if out.len() < code_len + value_bytes + stubs { return Err("lane body is larger than its placement".to_owned()) }
    let (code_out, veneer_out) = out.split_at_mut(code_len + value_bytes);
    // The value table follows the code in the same block, so its `adrp` is in range.
    let unresolved = place_values(image, vm, code_out, code_len)?;
    require_placed(&lane.code, &unresolved)?;
    // The symbols resolve to the table's slots, so a heap value's identity is the word it holds.
    let heap_values = placed_heap_values(image, code_out, code_len);
    let mut veneers = Veneers { base: address + (code_len + value_bytes) as u64, next: 0, out: veneer_out };
    let bases: Box<[u64]> = live.bases.iter().map(|base| *base as u64).collect();
    let places = Places { bases: &bases, helpers: live.helpers, invalidation: live.invalidation.address(), calls: live.calls.records(),
        values: address + code_len as u64, value_counts: (image.atoms.len(), image.heap_values.len(), image.shapes.len()) };
    let mut roots = FoldedRoots::default();
    let mut symbol = None;
    let code = lane.code.link_through(address, |namespace, key| {
        symbol = SymbolTable::decode(image, namespace, key);
        let word = resolve(image, vm, &places, namespace, key)?;
        if let Some(Symbol::HeapValue(heap)) = symbol {
            roots.embed_heap_identity(quench_runtime::Value::from_bits(heap_values[heap as usize]));
        }
        Some(word)
    }, |target| veneers.emit(target)).map_err(|error| format!("{error} [{symbol:?}]"))?;
    code_out[..code_len].copy_from_slice(&code);
    Ok(roots.settled())
}

/// The words the value table holds: one per atom, heap value and shape.
pub(crate) fn value_words(image: &AotImage) -> usize {
    image.atoms.len() + image.heap_values.len() + image.shapes.len()
}

/// The slot of value `index` (atom, then heap value, then shape) in `places`' table.
fn value_slot(places: &Places<'_>, index: u32) -> Option<u64> {
    let (atoms, heap, shapes) = places.value_counts;
    ((index as usize) < atoms + heap + shapes).then(|| places.values + u64::from(index) * std::mem::size_of::<u64>() as u64)
}

/// Fills the value table at `at`: each atom's id, heap value's bits and shape's id, as this VM
/// resolved them, so a linking lowering's `adrp` loads the word it names. A recipe the running
/// program has not made yet keeps `undefined` and is reported, so a body that names it is not
/// placed (`require_placed`) until the program makes it.
fn place_values(image: &AotImage, vm: &mut Vm, out: &mut [u8], at: usize) -> Result<Vec<(u32, u32)>, String> {
    let size = std::mem::size_of::<u64>();
    let table = out.get_mut(at..at + value_words(image) * size).ok_or("AOT image block is too small for its values")?;
    let mut words = table.chunks_exact_mut(size);
    for text in &image.atoms {
        words.next().expect("the table holds every atom").copy_from_slice(&u64::from(vm.intern_atom(text)).to_le_bytes());
    }
    let mut unresolved = Vec::new();
    for key in 0..image.heap_values.len() as u32 {
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

/// The heap values the table at `at` holds, as the bits a linked body loads from it: a root keeps
/// the object itself, not the address of the word that names it.
fn placed_heap_values(image: &AotImage, out: &[u8], at: usize) -> Box<[u64]> {
    let size = std::mem::size_of::<u64>();
    let first = at + image.atoms.len() * size;
    (0..image.heap_values.len()).map(|key| {
        let word = &out[first + key * size..first + (key + 1) * size];
        u64::from_le_bytes(word.try_into().expect("a value word is eight bytes"))
    }).collect()
}

/// Fails where `code` names a value `unresolved` (`(namespace, key)`) could not fill.
/// A guard of a recipe the program has not made yet is unresolved too: linked now, it would name
/// no shape and refuse every object the program makes later; linked when it is published, it
/// names the shape the program made (`symbols::guarded_shape`).
pub(crate) fn require_placed(code: &RelocatableCode, unresolved: &[(u32, u32)]) -> Result<(), String> {
    use super::super::compiled::RelocKind;
    let names = |relocation: &quench_runtime::vm::jit_backend::relocatable_code::Relocation, (namespace, key): (u32, u32)| (relocation.namespace == namespace && relocation.key == key)
        || (namespace == RelocKind::Shape as u32 && relocation.namespace == RelocKind::GuardShape as u32 && symbols::guard_recipe(relocation.key) == key);
    match unresolved.iter().copied().find(|&site| code.relocations.iter().any(|relocation| names(relocation, site)))
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
/// resolves when it is published (`link_lane_at`), so one that does not link yet is left unplaced.
fn place(image: &AotImage, vm: &mut Vm, layout: &Layout, (start, places): (usize, &Places<'_>),
    (out, linked): (&mut [u8], &mut [bool]), unresolved: &[(u32, u32)], veneers: &mut Veneers<'_>) -> Result<(), String> {
    let mut address = |namespace: u32, key: u32| resolve(image, vm, places, namespace, key);
    for (function, body) in image.functions.iter().enumerate() {
        link_into(out, start, layout.bases[function], &body.base.code, &mut address, veneers)?;
        if let (Some(lane), Some(at)) = (&body.lane, layout.lanes[function]) {
            linked[function] = require_placed(&lane.code, unresolved).is_ok()
                && link_into(out, start, at, &lane.code, &mut address, veneers).is_ok();
        }
    }
    Ok(())
}

/// Links `code` to run at `start + at` and copies it into `out` at `at`.
fn link_into(out: &mut [u8], start: usize, at: usize, code: &RelocatableCode, resolve: &mut impl FnMut(u32, u32) -> Option<u64>,
    veneers: &mut Veneers<'_>) -> Result<(), String> {
    let linked = code.link_through((start + at) as u64, &mut *resolve, |target| veneers.emit(target))?;
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
        Symbol::Atom(atom) => value_slot(places, image.atoms.get(atom as usize).map(|_| u32::from(atom))?),
        Symbol::HeapValue(key) => value_slot(places, u32::from(key) + image.atoms.len() as u32),
        Symbol::Shape(key) => value_slot(places, u32::from(key) + (image.atoms.len() + image.heap_values.len()) as u32),
        Symbol::Invalidation => Some(places.invalidation as u64),
        Symbol::NarrowUpperWords => Some(quench_runtime::value::NarrowLayout::upper_words() as u64),
        Symbol::Attention => Some(quench_runtime::execution_interrupt::attention_address()),
        Symbol::BasePc { function, pc } => base_pc(image, places.bases, function, pc),
        // A guard's shape is the immediate it compares (`object_slots::emit`), resolved in this VM.
        Symbol::GuardShape(_) => symbols::resolve_recipe(symbol, image, vm),
        Symbol::EntryCell(_) | Symbol::Descriptor(_) | Symbol::CaptureWords(_) | Symbol::ConstantPool(_) => None,
    }
}

/// The address of record `pc` of function `function`'s base body, if control enters it.
fn base_pc(image: &AotImage, bases: &[u64], function: u32, pc: u32) -> Option<u64> {
    let offset = *image.functions.get(function as usize)?.base.pc_offsets.get(pc as usize)?;
    if offset == NO_ENTRY { return None; }
    bases.get(function as usize).map(|base| base + u64::from(offset))
}

