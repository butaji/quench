//! The records of an image's proven calls (`AotImage::calls`) in this process, and their binding to
//! the closures the program makes. A record starts as the image's template, naming no callee
//! (`unbound_callee`), so its rows refuse every call to the VM's. It binds to a closure of the proven
//! function when a publication of its function's lane body resolves one (`Vm::install_ahead_body`),
//! and binds again when the VM's call of the site resolves another (`Vm::rebind_ahead_call`): the
//! guard's miss is the change, and the rows call the new closure by its entry from then on. A site
//! binds at most `POLYMORPHISM` closures, a site's feedback width; past it, the VM's call keeps it.
//!
//! Every closure bound stays alive and every descriptor written stays allocated for the image's
//! life: a record's identity is never a collected object's reused id, and a frame that names an
//! earlier descriptor (a callee running when its site rebinds) keeps reading a live one.
//!
//! Owner: one pipeline.

use super::{AotImage, unbound_callee};
use crate::folded_roots::{FoldedRoots, RootedCode};
use crate::vm::compiled::{tier_policy::POLYMORPHISM, DirectCallee};
use quench_bjit::call_record::CallRecord;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::rc::Rc;

/// The packed word's function bits (bits 0-31, `staged::frame`): the callee's function, which a binding names.
const FUNCTION_WORD: u64 = u32::MAX as u64;

/// The image's call records, by `Symbol::CallRecord` key, with what their bindings keep alive.
pub(crate) struct BoundCalls {
    /// The records the direct rows read in place.
    records: Records,
    /// Each record's key by its site, sorted: the VM's call of a site finds its record here.
    sites: Box<[((u32, u32), usize)]>,
    held: Rc<Held>,
    /// Whether the heap roots `held` (`Heap::hold_folded_roots`), which the first binding asks.
    rooted: Cell<bool>,
}

/// Where the records live: in the loader's own allocation (`aot_image::live::install`), or in the
/// executable's writable data, where the system linker placed them beside the code that reads them
/// (`aot_image::object`, `live::linked`).
enum Records {
    Owned(Box<[UnsafeCell<CallRecord>]>),
    Linked(&'static [UnsafeCell<CallRecord>]),
}

/// What the bindings keep: every closure bound, and by key every descriptor the record named.
struct Held {
    callees: RefCell<FoldedRoots>,
    descriptors: Box<[RefCell<Vec<Box<DirectCallee>>>]>,
}

impl RootedCode for Held {
    fn is_published(&self) -> bool { true }
    fn extend_roots(&self, roots: &mut Vec<crate::Value>) { self.callees.borrow().extend_roots(roots) }
}

impl BoundCalls {
    /// The records of `image`, each its template.
    pub(crate) fn of(image: &AotImage) -> Self {
        let records = Records::Owned(image.calls.iter().map(|call| UnsafeCell::new(call.record)).collect());
        Self::with_records(image, records)
    }

    /// The records of `image` the executable holds at `records`, each reset to its template.
    ///
    /// # Safety
    /// `records` is the image's writable call records record (`object::call_data`), one `CallRecord`
    /// per `image.calls`, aligned, alive for the process, and read by no code while it loads.
    pub(crate) unsafe fn linked(image: &AotImage, records: *mut CallRecord) -> Self {
        // SAFETY: the caller's contract; `UnsafeCell<CallRecord>` has `CallRecord`'s layout.
        let records: &'static [UnsafeCell<CallRecord>] = unsafe { std::slice::from_raw_parts(records.cast(), image.calls.len()) };
        // SAFETY: no code reads the records while the image loads.
        records.iter().zip(&image.calls).for_each(|(record, call)| unsafe { *record.get() = call.record });
        Self::with_records(image, Records::Linked(records))
    }

    fn with_records(image: &AotImage, records: Records) -> Self {
        let mut sites: Vec<((u32, u32), usize)> = image.calls.iter().enumerate().map(|(key, call)| ((call.function, call.pc), key)).collect();
        sites.sort_unstable();
        let descriptors = image.calls.iter().map(|_| RefCell::default()).collect();
        Self { records, sites: sites.into(), held: Rc::new(Held { callees: RefCell::default(), descriptors }), rooted: Cell::new(false) }
    }

    pub(crate) fn records(&self) -> &[UnsafeCell<CallRecord>] {
        match &self.records {
            Records::Owned(records) => records,
            Records::Linked(records) => records,
        }
    }

    /// The key of the record of the call at `pc` of `function`, if the image proves that call.
    pub(crate) fn key(&self, site: (u32, u32)) -> Option<usize> {
        self.sites.binary_search_by_key(&site, |(at, _)| *at).ok().map(|index| self.sites[index].1)
    }

    /// Whether record `key` names `callee`'s closure now.
    pub(crate) fn names(&self, key: usize, callee: crate::Value) -> bool {
        // SAFETY: the record is read on this thread, between runs of the code that reads it.
        self.records().get(key).is_some_and(|record| unsafe { (*record.get()).callee } == callee.to_bits())
    }

    /// Binds record `key` to `direct`, the callee its site resolved: the record's run-time words become
    /// the callee's, as a JIT plan's record holds them (`DirectFacts::of`), the descriptor is the callee's
    /// `DirectCallee` address (the native header's descriptor word), and the identity last, which admits
    /// the rows and the AOT lane body's native call (`call_frames::push_native`). `None` where the site has
    /// bound its `POLYMORPHISM` closures already.
    pub(crate) fn bind(&self, key: usize, direct: DirectCallee, heap: &mut crate::heap::Heap) -> Option<()> {
        let (record, mut held) = (self.records().get(key)?.get(), self.held.descriptors.get(key)?.borrow_mut());
        if held.len() >= POLYMORPHISM { return None; }
        if !self.rooted.replace(true) {
            let code: Rc<dyn RootedCode> = self.held.clone();
            heap.hold_folded_roots(&code);
        }
        let identity = self.held.callees.borrow_mut().embed_heap_identity(direct.callee);
        held.push(Box::new(direct));
        // The box's contents do not move when the vector grows, so the descriptor address stays valid.
        let direct: &DirectCallee = &**held.last()?;
        let validity = (direct.cell + crate::staged::call::ENTRY_CELL.validity_offset as usize) as *const u64;
        let descriptor = std::ptr::from_ref(direct) as u64;
        // SAFETY: the record is the image's, read in place by code on this thread, none of which runs
        // while the VM binds; the cell is the callee's live entry cell (`DirectCallee::cell`).
        unsafe {
            *record = CallRecord { callee: unbound_callee(), cell: direct.cell as u64, expected: validity.read(),
                call: direct.call as u64, caller_base: direct.caller_base as u64,
                packed: ((*record).packed & !FUNCTION_WORD) | u64::from(direct.target.function),
                captures: direct.capture_word(), global_this: direct.sloppy.map_or(0, crate::Value::to_bits), descriptor,
                env: direct.target.context_words().0, capture: direct.target.context_words().1,
                ..*record };
            (*record).callee = identity;
        }
        Some(())
    }
}
