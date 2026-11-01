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
    /// Subjects a dead prototype released, for reuse.
    free: Vec<u32>,
}

impl Table {
    fn retire(&mut self, subject: u32) {
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
    fn table(&self) -> &Table {
        // SAFETY: the table is reached only through this owner and the `Watch`es it hands
        // out, all on the heap's thread; neither keeps a reference across a call, as every
        // accessor here returns copies.
        unsafe { &*self.0.get() }
    }

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
