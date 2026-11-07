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

use crate::value::Value;
use std::cell::UnsafeCell;
use std::ptr::NonNull;

/// Prototypes an inherited field load may traverse to its holder, or to the end of the chain
/// for an absent name. Node's stream objects (an `IncomingMessage` above `Readable`, `Stream`,
/// `EventEmitter` and `Object.prototype`) sit five prototypes above their instances, and the
/// Babel parser's methods up to a dozen (its parser classes and plugin mixins); deeper chains
/// stay uncached.
pub(crate) const INHERITED_FIELD_CACHE_MAX_DEPTH: usize = 16;
/// A field load answered `depth` prototypes above a receiver of shape `receiver`: the chain
/// still has exactly `prototype_shapes[..depth]`, and `answer` says what that chain holds.
#[derive(Clone)]
pub(crate) struct InheritedFieldCache {
    pub(crate) receiver: u32,
    pub(crate) atom: crate::bytecode::Atom,
    pub(crate) prototype_shapes: [u32; INHERITED_FIELD_CACHE_MAX_DEPTH],
    pub(crate) depth: u8,
    /// Whether a function may stand on the chain (`Vm::function_links_answer` of the name).
    pub(crate) function_links: bool,
    pub(crate) answer: InheritedFieldAnswer,
    /// Whether a walk proved the chain from `proved_prototype` up to `proved_holder`, its
    /// last recorded prototype: the table clears it when any prototype the walk read
    /// changes or dies (`Dependencies::prove_inherited`), so it never outlives the record
    /// it names, and a use needs no walk while it holds.
    pub(crate) proved: bool,
    /// The receiver's prototype link the proof holds for, compared undecoded.
    pub(crate) proved_prototype: super::ProtoLink,
    pub(crate) proved_holder: Value,
    /// The value a `Slot` answer read under the current proof, held while `constant_watch`
    /// is published (`heap::watch_prototype_values` of the holder): a use then reads it
    /// without the holder. `None` before a read and once a walk proves the chain again.
    pub(crate) constant: Value,
    pub(crate) constant_watch: Option<crate::heap::PrototypeValueWatch>,
    /// The site's next older entry, or `NO_INHERITED_FIELD`: a site keeps up to
    /// `INHERITED_FIELD_CACHE_WAYS` receiver shapes, newest first.
    pub(crate) next: u32,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InheritedFieldAnswer {
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
}

/// The proofs the table holds, each cleared when a subject it read changes.
#[derive(Default)]
pub(crate) struct Proofs {
    pub(crate) index_free_chain: Option<IndexFreeChainProof>,
}

impl Proofs {
    fn retire(&mut self, kind: ProofKind) {
        match kind {
            ProofKind::IndexFreeChain => self.index_free_chain = None,
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
    /// Subjects a dead prototype released, for reuse.
    free: Vec<u32>,
}

impl Table {
    fn retire(&mut self, subject: u32) {
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

    /// Clears every proof of `kind`; the subjects that recorded a reader of it keep the
    /// bit, which a later retirement clears with nothing left to clear.
    fn retire_kind(&mut self, kind: ProofKind) {
        self.proofs.retire(kind);
    }
}

/// How many times its entries a subject's inherited reader list may hold before it is
/// deduplicated (`Dependencies::prove_inherited`).
const INHERITED_READERS_SLACK: usize = 2;

/// The heap's dependency table.
pub(crate) struct Dependencies(Box<UnsafeCell<Table>>);

impl Default for Dependencies {
    fn default() -> Self {
        Self(Box::new(UnsafeCell::new(Table::default())))
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

    /// The subject of a prototype that died: its proofs retire, and the subject is free.
    pub(crate) fn release(&mut self, subject: u32) {
        let table = self.table_mut();
        table.retire(subject);
        table.free.push(subject);
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

    /// Drops every inherited entry's proof: the caches were cleared.
    pub(crate) fn clear_inherited(&mut self) {
        let table = self.table_mut();
        table.inherited.clear();
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
            unsafe { (*table.as_ref().get()).retire(subject) };
        }
    }
}
