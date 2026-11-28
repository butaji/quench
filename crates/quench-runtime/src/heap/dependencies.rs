//! The facts cached proofs rely on, and the events that retire them (R2b, TOD-1248).
//!
//! A *subject* is one watched prototype: its layout (shape, prototype link, integrity
//! bits, recorded descriptors) and, for an array, its elements. A *proof* is a fact a
//! cache holds about one or more subjects. Proving one records it on every subject it
//! read; a change to a subject retires each proof that read it by clearing the proof.
//! So a use reads a held proof with no epoch compare and no walk, and the work of a
//! change is proportional to the proofs that read the changed subject.
//!
//! The table lives in its own allocation, which the heap owns and each watched
//! prototype's extras point to (`Watch`): a layout change, made through the prototype's
//! own `Object` methods, retires its subject without reaching the heap.
//!
//! A proof held outside the table, in a cache of plain words, keeps the validity cell of the
//! chain it read (`ChainCell`), which every subject of the chain lists: a change to one of
//! them, or its death, retires the cell, and a use is one load and one compare. So a change to
//! a prototype re-proves only what depends on it, never every prototype-dependent fact in the
//! VM. The events no subject names (a collection that may reuse a dead prototype's record, a
//! shape compaction, a restore) retire every cell (`Dependencies::retire_cells`) with the
//! global `prototype_layout_epoch`. A dependency object, or compiled code that took a fact read
//! through a chain, may hold the same subjects (`Dependencies::hold_layouts`).

use crate::typed_ir::FactKey;
use crate::value::Value;
use std::cell::UnsafeCell;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

/// A prototype chain's validity cell (as V8's prototype validity cells): it holds while every
/// prototype of one chain keeps the layout it had when the cell was made
/// (`Dependencies::chain_cell`), and retires for good at the first change to one of them, to
/// any one's death, or at an event no subject names (`Dependencies::retire_cells`). A proof
/// made about a chain keeps the chain's cell, so a change to a prototype retires only the
/// proofs whose chain holds it: a prototype built elsewhere leaves them standing.
///
/// A copy of plain words, so a proof held in a direct-mapped table keeps it as it kept an
/// epoch, and a use is one load and one compare: the cell's word still holds `id`. Words live
/// in a thread's slab (`CELL_SLAB`) that never frees them, so a cell outlives any table; a
/// retired word is reused under a new id, which no earlier cell names.
#[derive(Clone, Copy)]
pub(crate) struct ChainCell {
    word: &'static AtomicU64,
    id: u64,
}

impl ChainCell {
    /// Whether every prototype of the cell's chain keeps the layout it was made under.
    #[inline(always)]
    pub(crate) fn holds(self) -> bool {
        self.word.load(Ordering::Relaxed) == self.id
    }

    /// A new cell, holding until retired.
    fn new() -> Self {
        let word = CELL_SLAB.try_with(|slab| {
            let mut free = slab.borrow_mut();
            if free.is_empty() {
                let chunk: &'static [AtomicU64] = (0..CELL_CHUNK).map(|_| AtomicU64::new(0)).collect::<Vec<_>>().leak();
                free.extend(chunk.iter());
            }
            free.pop()
        });
        // A slab that cannot be reached (a thread exiting) gives a word of its own.
        let word: &'static AtomicU64 = match word {
            Ok(Some(word)) => word,
            _ => Box::leak(Box::new(AtomicU64::new(0))),
        };
        let id = NEXT_CELL_ID.fetch_add(1, Ordering::Relaxed);
        word.store(id, Ordering::Relaxed);
        Self { word, id }
    }

    /// Retires the cell, whose word then serves another; a retired cell stays retired. Whether
    /// it held.
    fn retire(self) -> bool {
        let held = self.holds();
        if held {
            self.word.store(0, Ordering::Relaxed);
            // A slab gone with its thread only loses the word.
            let _ = CELL_SLAB.try_with(|slab| slab.borrow_mut().push(self.word));
        }
        held
    }
}

/// Cells are equal when they are one cell: ids are never reused.
impl PartialEq for ChainCell {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ChainCell {}

impl std::fmt::Debug for ChainCell {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("ChainCell").field(&self.id).field(&self.holds()).finish()
    }
}

/// Words a slab allocates at once (`ChainCell::new`).
const CELL_CHUNK: usize = 256;

/// The next cell id: process-wide, so no cell of any table or thread repeats another's id.
static NEXT_CELL_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// The words no holding cell names, for new cells (`ChainCell`).
    static CELL_SLAB: std::cell::RefCell<Vec<&'static AtomicU64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The length past which a subject's cell list drops the cells already retired, at each power
/// of two, amortized over the pushes that grew it (`Dependencies::chain_cell`).
const CELL_READERS_PRUNE: usize = 8;

/// Prototypes an inherited field load may traverse to its holder, or to the end of the chain
/// for an absent name. Node's stream objects (an `IncomingMessage` above `Readable`, `Stream`,
/// `EventEmitter` and `Object.prototype`) sit five prototypes above their instances, and the
/// Babel parser's methods up to a dozen (its parser classes and plugin mixins); deeper chains
/// stay uncached.
pub const INHERITED_FIELD_CACHE_MAX_DEPTH: usize = 16;
/// A field load answered `depth` prototypes above a receiver of shape `receiver`: the chain
/// still has exactly the `depth` prototype shapes interned at `chain`
/// (`Dependencies::inherited_chain`), and `answer` says what that chain holds.
///
/// The shapes are interned, not held inline: every site reading through one class
/// hierarchy names the same chain, and an inline array of `INHERITED_FIELD_CACHE_MAX_DEPTH`
/// shapes was 64 of the entry's 120 bytes. An entry is now 56: holder, constant and watch
/// (24), receiver, atom, chain, proved prototype, proved layout and next (24), answer (4),
/// depth, function links and proved (3), padded.
#[derive(Clone)]
pub struct InheritedFieldCache {
    pub receiver: u32,
    pub atom: crate::bytecode::Atom,
    /// Where the chain's prototype shapes start in the interned chains
    /// (`Dependencies::intern_inherited_chain`).
    pub chain: u32,
    pub depth: u8,
    /// Whether a function may stand on the chain (`Vm::function_links_answer` of the name).
    pub function_links: bool,
    pub answer: InheritedFieldAnswer,
    /// Whether a walk proved the chain from `proved_prototype` up to `proved_holder`, its
    /// last recorded prototype: the table clears it when any prototype the walk read
    /// changes or dies (`Dependencies::prove_inherited`), so it never outlives the record
    /// it names, and a use needs no walk while it holds.
    pub proved: bool,
    /// The receiver's prototype link the proof holds for, compared undecoded.
    pub proved_prototype: super::ProtoLink,
    /// The layout id (`heap::layouts`) of a receiver the proof last answered, or
    /// `NO_PROVED_LAYOUT`: its header word holds the receiver shape and the prototype link,
    /// so a receiver of that layout matches both on one compare. Reset with each new proof.
    pub proved_layout: u32,
    pub proved_holder: Value,
    /// The value a `Slot` answer read under the current proof, held while `constant_watch`
    /// is published (`heap::watch_prototype_values` of the holder): a use then reads it
    /// without the holder. `None` before a read and once a walk proves the chain again.
    pub constant: Value,
    pub constant_watch: Option<crate::heap::PrototypeValueWatch>,
    /// The site's next older entry, or `NO_INHERITED_FIELD`: a site keeps up to
    /// `INHERITED_FIELD_CACHE_WAYS` receiver shapes, newest first.
    pub next: u32,
}
/// Shape chains interned once each, by content: a cache entry names its chain by the
/// offset of its first shape, and keeps its length itself. Stored as each chain's length,
/// then its shapes, with a hash table of the offsets. Equal chains intern to one offset, so
/// two entries' chains are equal exactly when their offsets and lengths are.
#[derive(Default)]
pub(crate) struct InternedChains {
    words: Vec<u32>,
    index: hashbrown::HashTable<u32>,
}

impl InternedChains {
    fn at(words: &[u32], offset: u32) -> &[u32] {
        let offset = offset as usize;
        let len = words[offset - 1] as usize;
        &words[offset..offset + len]
    }

    fn hash(shapes: &[u32]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        shapes.hash(&mut hasher);
        hasher.finish()
    }

    /// The offset of `shapes`, interned first where no chain held them yet; an empty chain
    /// names offset zero and holds nothing.
    pub(crate) fn intern(&mut self, shapes: &[u32]) -> u32 {
        if shapes.is_empty() {
            return 0;
        }
        let hash = Self::hash(shapes);
        let words = &self.words;
        if let Some(&offset) = self.index.find(hash, |&offset| Self::at(words, offset) == shapes) {
            return offset;
        }
        self.words.push(shapes.len() as u32);
        let offset = u32::try_from(self.words.len()).expect("interned chains fit u32");
        self.words.extend_from_slice(shapes);
        let words = &self.words;
        self.index.insert_unique(hash, offset, |&offset| Self::hash(Self::at(words, offset)));
        offset
    }

    /// The `len` shapes interned at `offset`; empty for an offset no chain of that length
    /// starts at.
    #[inline(always)]
    pub(crate) fn get(&self, offset: u32, len: usize) -> &[u32] {
        self.words.get(offset as usize..).and_then(|chain| chain.get(..len)).unwrap_or(&[])
    }

    /// Drops every chain: no entry names one any more.
    pub(crate) fn clear(&mut self) {
        self.words.clear();
        self.index.clear();
    }

    /// Bytes held: the words and the index's slots and control bytes.
    #[cfg(feature = "profile-memory")]
    pub(crate) fn bytes(&self) -> usize {
        self.words.capacity() * size_of::<u32>() + self.index.capacity() * (size_of::<u32>() + 1)
    }
}

/// An `InheritedFieldCache::proved_layout` no layout id reaches.
pub(crate) const NO_PROVED_LAYOUT: u32 = u32::MAX;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InheritedFieldAnswer {
    /// The last recorded prototype holds the data property at this slot.
    Slot(u16),
    /// No recorded shape has the name and the last prototype's prototype is null: the load
    /// is `undefined`.
    Absent,
}

/// A kind of cached proof, by its bit in a subject's `readers`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ProofKind {
    /// No prototype from `IndexFreeChainProof::first` on holds an array index.
    IndexFreeChain,
}

/// That no prototype on the chain from `first` holds an array index property: no indexed
/// descriptor or index key, no element (an array holder's dense or sparse), and none of
/// them exotic. Held while every prototype it read keeps its layout and elements.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IndexFreeChainProof {
    pub(crate) first: Value,
    /// `first`'s prototype, whose chain the proof covers as well: a watched `first` keeps it.
    pub(crate) next: Value,
    /// `first` and `next` as prototype links (`ProtoLink::bits`), which a holder's header
    /// matches without decoding its link.
    pub(crate) links: [u32; 2],
}

impl IndexFreeChainProof {
    pub(crate) fn new(first: Value, next: Value) -> Self {
        let links = [first, next].map(|proto| super::cell::ProtoLink::of(proto).bits());
        Self { first, next, links }
    }
}

/// The proofs the table holds, each cleared when a subject it read changes.
#[derive(Default)]
pub(crate) struct Proofs {
    pub(crate) index_free_chain: Option<IndexFreeChainProof>,
}

impl Proofs {
    fn retire(&mut self, kind: ProofKind) {
        match kind {
            ProofKind::IndexFreeChain => {
                self.index_free_chain = None;
                super::set_index_free_chain_link(super::NO_INDEX_FREE_CHAIN_LINK);
            }
        }
    }
}

/// The proof kinds by bit, each kind's bit its discriminant.
const PROOF_KINDS: [ProofKind; 1] = [ProofKind::IndexFreeChain];

#[derive(Default)]
struct Table {
    proofs: Proofs,
    /// Per subject, the kinds of the proofs that read it, as bits.
    readers: Vec<u32>,
    /// Per subject, the inherited field cache entries whose chain proof read it.
    inherited_readers: Vec<Vec<u32>>,
    /// The inherited field cache's entries (`Vm::inherited_field_indices`), each of whose
    /// chain proofs the table retires.
    inherited: Vec<InheritedFieldCache>,
    /// The prototype-shape chains inherited entries name (`InheritedFieldCache::chain`).
    inherited_chains: InternedChains,
    /// Per subject, the fact-table entries (`FactStore`) whose answer read it.
    fact_readers: Vec<Vec<u32>>,
    /// The fact table whose entries a retirement clears.
    fact_store: Option<FactStore>,
    /// Subjects a dead prototype released, for reuse.
    free: Vec<u32>,
    /// The intrinsic-builtin fuses, which watched prototypes hold.
    fuses: crate::code_dependencies::Fuses,
    /// Per subject, the chain cells whose chain holds it (`ChainCell`).
    cell_readers: Vec<Vec<ChainCell>>,
    /// Per subject, the dependencies held on its layout (`Dependencies::hold_layouts`): data
    /// dependencies and compiled code. A retirement takes them out (`Retired`) and retires
    /// them once it holds the table no longer, so code dropped then never finds it borrowed.
    layout_readers: crate::code_dependencies::CodeReaders,
    /// Per fact cell key, the code that trusts the cell (`typed_ir::FactKey`). An edge that widens
    /// the cell takes them out and retires them once the table is released (`Table::widen_cell`).
    cell_key_readers: crate::code_dependencies::CellReaders,
    /// The cell last made for each chain, by its first subject and length: every proof about
    /// one chain shares it while it holds. A chain is its first prototype's links, so while no
    /// subject of the cell changed, the chain of that length from its first subject is the one
    /// the cell was made for.
    chain_cells: rustc_hash::FxHashMap<(u32, usize), ChainCell>,
}

impl Drop for Table {
    /// Retires every cell the table made, whose words return to the slab, and what held a layout.
    fn drop(&mut self) {
        self.retire_cells().retire();
    }
}

/// The dependencies a retirement took out of the table (`Table::layout_readers`), to retire once
/// the table is no longer borrowed.
#[must_use]
struct Retired(Vec<std::rc::Rc<dyn crate::code_dependencies::DependentCode>>);

impl Retired {
    fn retire(self) {
        self.0.into_iter().for_each(|dependency| dependency.retire());
    }
}

impl Table {
    /// Retires what read `subject`; what held its layout is given back to retire (`Retired`).
    fn retire(&mut self, subject: u32) -> Retired {
        let held = Retired(self.layout_readers.take(subject));
        self.retire_proofs(subject);
        held
    }

    fn retire_proofs(&mut self, subject: u32) {
        self.fuses.note_layout_change(subject);
        if let Some(cells) = self.cell_readers.get_mut(subject as usize) {
            for cell in std::mem::take(cells) {
                cell.retire();
            }
        }
        if let (Some(store), Some(entries)) = (self.fact_store, self.fact_readers.get_mut(subject as usize)) {
            for entry in std::mem::take(entries) {
                store.clear(entry);
            }
        }
        if let Some(entries) = self.inherited_readers.get_mut(subject as usize) {
            for entry in std::mem::take(entries) {
                if let Some(cache) = self.inherited.get_mut(entry as usize) {
                    cache.proved = false;
                }
            }
        }
        let Some(readers) = self.readers.get_mut(subject as usize) else { return };
        let kinds = std::mem::take(readers);
        if kinds == 0 {
            return;
        }
        for kind in PROOF_KINDS {
            if kinds & 1 << kind as u32 != 0 {
                self.retire_kind(kind);
            }
        }
    }

    /// Lists `cell` as reading `subject`, first dropping the list's retired cells where it
    /// reaches a power of two past `CELL_READERS_PRUNE`, amortized over the pushes that grew it.
    fn push_cell_reader(&mut self, subject: u32, cell: ChainCell) {
        let cells = &mut self.cell_readers[subject as usize];
        if cells.len() >= CELL_READERS_PRUNE && cells.len().is_power_of_two() {
            cells.retain(|cell| cell.holds());
        }
        cells.push(cell);
    }

    /// Retires every chain cell and gives every layout holder to retire: an event no subject
    /// names (a collection that may reuse a dead prototype's record, a shape compaction, a restore).
    fn retire_cells(&mut self) -> Retired {
        self.chain_cells.drain().for_each(|(_, cell)| {
            cell.retire();
        });
        self.cell_readers.iter_mut().flat_map(std::mem::take).for_each(|cell| {
            cell.retire();
        });
        Retired(self.layout_readers.take_all())
    }

    /// The code that trusts cell `key` (`cell_key_readers`), to retire once the table is released.
    fn widen_cell(&mut self, key: FactKey) -> Retired {
        Retired(self.cell_key_readers.take(key))
    }

    /// The code that trusts any cell, for an event no key names (`Dependencies::widen_all_cells`).
    fn widen_all_cells(&mut self) -> Retired {
        Retired(self.cell_key_readers.take_all())
    }

    /// Clears every proof of `kind`; the subjects that recorded a reader of it keep the
    /// bit, which a later retirement clears with nothing left to clear.
    fn retire_kind(&mut self, kind: ProofKind) {
        self.proofs.retire(kind);
    }
}

/// How many times its entries a subject's inherited reader list may hold before it is
/// deduplicated (`Dependencies::prove_inherited`).
const INHERITED_READERS_SLACK: usize = 2;

/// The fact table's entries as a retirement sees them: 16-byte entries whose first word is
/// the tag, zero for an empty one (`vm::symbol_load_proof`). A retirement zeroes the tag of
/// each entry a subject's reader list names, so a later probe misses on its tag compare and
/// pays nothing for the retirement; an entry since reused for another fact is cleared as
/// well, which only costs that fact a walk.
#[derive(Clone, Copy)]
pub(crate) struct FactStore {
    entries: NonNull<[std::cell::Cell<u64>; 2]>,
    count: u32,
}

impl FactStore {
    /// The store whose `count` entries start at `entries`, which stay mapped for as long as
    /// the heap does.
    pub(crate) fn new(entries: NonNull<[std::cell::Cell<u64>; 2]>, count: u32) -> Self {
        Self { entries, count }
    }

    fn clear(self, entry: u32) {
        if entry < self.count {
            // SAFETY: `entry` is below the mapping's entry count (`new`).
            unsafe { (*self.entries.as_ptr().add(entry as usize))[0].set(0) };
        }
    }
}

/// The heap's dependency table.
#[quench_macros::test_visible]
pub(crate) struct Dependencies(Box<UnsafeCell<Table>>);

impl Default for Dependencies {
    fn default() -> Self {
        Self(Box::new(UnsafeCell::new(Table::default())))
    }
}

impl Drop for Dependencies {
    /// Retires the held proofs: the thread's link word mirrors them, and outlives this heap.
    fn drop(&mut self) {
        self.table_mut().retire_kind(ProofKind::IndexFreeChain);
    }
}

impl std::fmt::Debug for Dependencies {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Dependencies").finish_non_exhaustive()
    }
}

impl Dependencies {
    #[inline(always)]
    fn table(&self) -> &Table {
        // SAFETY: the table is reached only through this owner and the `Watch`es it hands
        // out, all on the heap's thread; neither keeps a reference across a call, as every
        // accessor here returns copies.
        unsafe { &*self.0.get() }
    }

    #[inline(always)]
    fn table_mut(&mut self) -> &mut Table {
        self.0.get_mut()
    }

    /// The held index-free chain proof, if any (`ProofKind::IndexFreeChain`).
    #[inline(always)]
    pub(crate) fn index_free_chain(&self) -> Option<IndexFreeChainProof> {
        self.table().proofs.index_free_chain
    }

    /// A subject for a newly watched prototype, and the watch its extras keep.
    pub(crate) fn watch(&mut self) -> Watch {
        let table = self.table_mut();
        let subject = match table.free.pop() {
            Some(subject) => subject,
            None => {
                table.readers.push(0);
                table.inherited_readers.push(Vec::new());
                table.fact_readers.push(Vec::new());
                table.cell_readers.push(Vec::new());
                (table.readers.len() - 1) as u32
            }
        };
        Watch(Some((NonNull::from(&*self.0).cast(), subject)))
    }

    /// Records `proof` of `kind` as read from each of `subjects`, and holds it. A proof is
    /// made while the code that reads it holds no reference into the table, as a layout
    /// change's retirement does (`Watch::retire`).
    pub(crate) fn prove(&self, kind: ProofKind, subjects: &[u32], proof: impl FnOnce(&mut Proofs)) {
        // SAFETY: as `table`: no reference to the table is live across this call.
        let table = unsafe { &mut *self.0.get() };
        for subject in subjects {
            table.readers[*subject as usize] |= 1 << kind as u32;
        }
        proof(&mut table.proofs);
    }

    #[quench_macros::test_visible]
    /// The subject of a prototype that died: its proofs retire, and the subject is free.
    pub(crate) fn release(&mut self, subject: u32) {
        let table = self.table_mut();
        let held = table.retire(subject);
        table.fuses.forget(subject);
        table.free.push(subject);
        held.retire();
    }

    /// The bytes of the layouts code holds on this heap's subjects (`hold_layouts`).
    pub(crate) fn layout_hold_bytes(&self) -> usize {
        self.table().layout_readers.memory_bytes()
    }

    /// Retires every proof, for changes no subject names (a restored heap's prototypes).
    pub(crate) fn retire_all(&mut self) {
        let table = self.table_mut();
        for kind in PROOF_KINDS {
            table.retire_kind(kind);
        }
        table.readers.iter_mut().for_each(|readers| *readers = 0);
        table.inherited_readers.iter_mut().for_each(Vec::clear);
        table.inherited.iter_mut().for_each(|cache| cache.proved = false);
        if let Some(store) = table.fact_store {
            table.fact_readers.iter_mut().flat_map(std::mem::take).for_each(|entry| store.clear(entry));
        }
        table.fuses.forget_all();
        table.retire_cells().retire();
    }

    #[quench_macros::test_visible]
    /// Retires every chain cell (`Table::retire_cells`), for an event no subject names: a
    /// collection that may let a dead prototype's record be reused, or a shape compaction,
    /// which renumbers the shapes proofs name.
    pub(crate) fn retire_cells(&mut self) {
        self.table_mut().retire_cells().retire();
    }

    /// The cell of the chain whose prototypes' subjects are `subjects`, nearest first: one
    /// that holds while none of them changes or dies (`ChainCell`). Every proof about one chain
    /// shares its cell; a cell for no prototype holds until an event no subject names.
    pub(crate) fn chain_cell(&self, subjects: &[u32]) -> ChainCell {
        // SAFETY: as `table`: no reference to the table is live across this call.
        let table = unsafe { &mut *self.0.get() };
        let key = (subjects.first().copied().unwrap_or(u32::MAX), subjects.len());
        if let Some(cell) = table.chain_cells.get(&key).copied().filter(|cell| cell.holds()) {
            return cell;
        }
        let cell = ChainCell::new();
        for subject in subjects {
            table.push_cell_reader(*subject, cell);
        }
        table.chain_cells.insert(key, cell);
        cell
    }

    /// Makes `dependency` retire when one of `subjects` changes its layout or dies, or at an
    /// event no subject names (`retire_cells`); `false` where the reader table is full. A data
    /// dependency (`code_dependencies::new_dependency`) or compiled code (`code_dependencies::PrototypeChain`): it
    /// retires after the table is released (`Retired`).
    pub(crate) fn hold_layouts(&self, subjects: &[u32], dependency: &std::rc::Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
        // SAFETY: as `table`: no reference to the table is live across this call.
        let table = unsafe { &mut *self.0.get() };
        subjects.iter().all(|subject| table.layout_readers.subscribe(*subject, dependency))
    }

    /// Makes `code` retire when cell `key` widens (`widen_cell`); `false` where the reader table is
    /// full. Code that trusts a cell subscribes here at the cell's key, not per slot or class.
    pub(crate) fn subscribe_cell(&mut self, key: FactKey, code: &std::rc::Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
        self.table_mut().cell_key_readers.subscribe(key, code)
    }

    /// An edge moved cell `key` up the lattice (a store outside the cell, a method slot write, a
    /// global write, a length change, new code). Every reader is retired the same way in every
    /// tier: records requicken, compiled units retire. This is the natural transformation on code
    /// as a presheaf over cells: the edge names the key, and the readers of that key retire.
    /// The readers leave the table before they retire, so retiring never meets a borrow of it.
    pub(crate) fn widen_cell(&mut self, key: FactKey) {
        let retired = self.table_mut().widen_cell(key);
        retired.retire();
    }

    /// `widen_cell` of every cell, for an event no key names: new code reaching the scopes (eval).
    pub(crate) fn widen_all_cells(&mut self) {
        let retired = self.table_mut().widen_all_cells();
        retired.retire();
    }

    /// A cell of its own that holds while none of `subjects`, prototypes of any relation,
    /// changes or dies: for a proof that read several objects' layouts, not one chain. With no
    /// subject, the shared cell of no chain (`chain_cell`), which an event no subject names
    /// retires.
    pub(crate) fn subjects_cell(&self, subjects: &[u32]) -> ChainCell {
        if subjects.is_empty() {
            return self.chain_cell(subjects);
        }
        // SAFETY: as `table`: no reference to the table is live across this call.
        let table = unsafe { &mut *self.0.get() };
        let cell = ChainCell::new();
        for subject in subjects {
            table.push_cell_reader(*subject, cell);
        }
        cell
    }

    /// The intrinsic-builtin fuses (`code_dependencies::Fuses`).
    #[inline(always)]
    pub(crate) fn fuses(&self) -> &crate::code_dependencies::Fuses {
        &self.table().fuses
    }

    pub(crate) fn fuses_mut(&mut self) -> &mut crate::code_dependencies::Fuses {
        &mut self.table_mut().fuses
    }

    /// Inherited field cache entry `entry`.
    ///
    /// # Safety
    /// `entry` is an index `push_inherited` returned since the last `clear_inherited`.
    #[inline(always)]
    pub(crate) unsafe fn inherited_unchecked(&self, entry: u32) -> &InheritedFieldCache {
        // SAFETY: as the caller promises.
        unsafe { self.table().inherited.get_unchecked(entry as usize) }
    }

    #[inline(always)]
    pub(crate) fn inherited(&self, entry: u32) -> &InheritedFieldCache {
        &self.table().inherited[entry as usize]
    }

    /// Inherited field cache entry `entry`, where the cache holds one.
    #[inline(always)]
    pub(crate) fn inherited_get(&self, entry: u32) -> Option<&InheritedFieldCache> {
        self.table().inherited.get(entry as usize)
    }

    pub(crate) fn inherited_mut(&mut self, entry: u32) -> &mut InheritedFieldCache {
        &mut self.table_mut().inherited[entry as usize]
    }

    /// How many inherited entries the cache holds.
    #[cfg(test)]
    pub(crate) fn inherited_count(&self) -> usize {
        self.table().inherited.len()
    }

    /// The offset of `shapes` among the interned chains, interned first where no entry named
    /// it yet; an empty chain names none. Chains repeat across sites and receiver shapes
    /// (one class hierarchy's), so the table grows by the distinct chains only, however
    /// often a site records.
    pub(crate) fn intern_inherited_chain(&mut self, shapes: &[u32]) -> u32 {
        self.table_mut().inherited_chains.intern(shapes)
    }

    /// The prototype shapes `cache`'s chain names, nearest first.
    #[inline(always)]
    pub(crate) fn inherited_chain(&self, cache: &InheritedFieldCache) -> &[u32] {
        self.table().inherited_chains.get(cache.chain, usize::from(cache.depth))
    }

    /// Adds `cache` as a new inherited entry, and its index.
    pub(crate) fn push_inherited(&mut self, cache: InheritedFieldCache) -> u32 {
        let inherited = &mut self.table_mut().inherited;
        inherited.push(cache);
        (inherited.len() - 1) as u32
    }

    /// Holds inherited field cache entry `entry`'s chain proof, as read from `subjects`.
    pub(crate) fn prove_inherited(&mut self, entry: u32, subjects: &[u32]) {
        let table = self.table_mut();
        let entries = table.inherited.len();
        for subject in subjects {
            let readers = &mut table.inherited_readers[*subject as usize];
            readers.push(entry);
            // An entry recached and proved again without its subjects changing is listed
            // again: past twice the entries, the list is deduplicated, amortized over the
            // pushes that grew it.
            if readers.len() > INHERITED_READERS_SLACK * entries + INHERITED_READERS_SLACK {
                readers.sort_unstable();
                readers.dedup();
            }
        }
        table.inherited[entry as usize].proved = true;
    }

    /// Holds fact-table entry `entry` of `store` as read from `subjects`: a change to any of
    /// them clears it (`Table::retire`). A list past twice the store's entries is
    /// deduplicated, amortized over the pushes that grew it.
    pub(crate) fn prove_fact(&self, store: FactStore, entry: u32, subjects: &[u32]) {
        // SAFETY: as `table`: no reference to the table is live across this call.
        let table = unsafe { &mut *self.0.get() };
        table.fact_store = Some(store);
        for subject in subjects {
            let readers = &mut table.fact_readers[*subject as usize];
            readers.push(entry);
            if readers.len() > INHERITED_READERS_SLACK * store.count as usize {
                readers.sort_unstable();
                readers.dedup();
            }
        }
    }

    /// Drops every inherited entry's proof: the caches were cleared.
    pub(crate) fn clear_inherited(&mut self) {
        let table = self.table_mut();
        table.inherited.clear();
        table.inherited_chains.clear();
        table.inherited_readers.iter_mut().for_each(Vec::clear);
    }
}

/// A watched prototype's subject in its heap's table (`Dependencies::watch`), kept in the
/// prototype's extras. A copy of the object is no watched prototype: it watches nothing.
#[derive(Default)]
pub(crate) struct Watch(Option<(NonNull<UnsafeCell<Table>>, u32)>);

impl Clone for Watch {
    fn clone(&self) -> Self {
        Self(None)
    }
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("Watch").field(&self.subject()).finish()
    }
}

impl Watch {
    pub(crate) fn subject(&self) -> Option<u32> {
        self.0.map(|(_, subject)| subject)
    }

    /// Retires the proofs that read this subject: its layout or elements change.
    pub(crate) fn retire(&self) {
        if let Some((table, subject)) = self.0 {
            // SAFETY: the table outlives every object of its heap, which owns it, and no
            // reference to it is live while an object's layout changes (`Dependencies`).
            let held = unsafe { (*table.as_ref().get()).retire(subject) };
            held.retire();
        }
    }

    /// Pops the fuses this subject holds at `slot`: a write stores another value there.
    #[inline(always)]
    pub(crate) fn note_slot_write(&self, slot: usize) {
        if let Some((table, subject)) = self.0 {
            // SAFETY: as `retire`.
            unsafe { (*table.as_ref().get()).fuses.note_slot_write(subject, slot) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three watched prototypes: `parent`, `child` above it, and `other` on no chain of theirs.
    fn three() -> (Dependencies, [Watch; 3]) {
        let mut dependencies = Dependencies::default();
        let watches = [dependencies.watch(), dependencies.watch(), dependencies.watch()];
        (dependencies, watches)
    }

    #[test]
    fn a_cell_retires_at_a_change_on_its_chain_only() {
        let (dependencies, [child, parent, other]) = three();
        let subject = |watch: &Watch| watch.subject().unwrap();
        let chain = dependencies.chain_cell(&[subject(&child), subject(&parent)]);
        let parent_only = dependencies.chain_cell(&[subject(&parent)]);
        let other_only = dependencies.chain_cell(&[subject(&other)]);
        assert_eq!(dependencies.chain_cell(&[subject(&child), subject(&parent)]), chain, "one chain shares one cell");
        other.retire();
        assert!(chain.holds() && parent_only.holds(), "a prototype on neither chain retires neither");
        assert!(!other_only.holds());
        child.retire();
        assert!(!chain.holds(), "a change to the chain's first prototype retires it");
        assert!(parent_only.holds(), "a change below the parent keeps the parent's own chain");
        let chain = dependencies.chain_cell(&[subject(&child), subject(&parent)]);
        assert!(chain.holds() && chain != parent_only, "a retired chain takes a new cell");
        parent.retire();
        assert!(!chain.holds() && !parent_only.holds(), "a change to the parent retires every chain through it");
    }

    #[test]
    fn a_dying_prototype_and_a_global_event_retire_cells_and_layout_holds() {
        let (mut dependencies, [child, parent, other]) = three();
        let subject = |watch: &Watch| watch.subject().unwrap();
        let chain = dependencies.chain_cell(&[subject(&child), subject(&parent)]);
        let set = dependencies.subjects_cell(&[subject(&other), subject(&parent)]);
        let held = crate::code_dependencies::new_dependency();
        let dependency: std::rc::Rc<dyn crate::code_dependencies::DependentCode> = held.clone();
        assert!(dependencies.hold_layouts(&[subject(&other)], &dependency));
        dependencies.release(subject(&child));
        assert!(!chain.holds(), "a dead prototype's chains retire, so no proof names its reused record");
        assert!(set.holds() && held.is_published());
        let none = dependencies.chain_cell(&[]);
        assert!(none.holds(), "the cell of no chain holds until an event no subject names");
        dependencies.retire_cells();
        assert!(!set.holds() && !none.holds() && !held.is_published(), "a global event retires every cell");
        let fresh = dependencies.subjects_cell(&[subject(&other)]);
        other.retire();
        assert!(!fresh.holds(), "a subjects cell retires at a change to any of its subjects");
    }

    #[test]
    fn a_retired_cells_word_serves_a_new_cell_under_a_new_id() {
        let (dependencies, [child, ..]) = three();
        let first = dependencies.subjects_cell(&[child.subject().unwrap()]);
        child.retire();
        let second = dependencies.subjects_cell(&[child.subject().unwrap()]);
        assert!(!first.holds() && second.holds(), "a reused word holds only for the cell that took it");
    }
}
