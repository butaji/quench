//! The records of an image's proven calls (`AotImage::calls`) in this process, and their binding to
//! the closures the program makes. A record starts as the image's template, naming the unbound callee record
//! (`unbound_callee_record`), so its rows refuse every call to the VM's. A binding makes a callee record for the
//! closure (`CalleeRecord`) and names it from the site (`CallRecord::callee_record`). It binds to a closure of the proven
//! function when a publication of its function's lane body resolves one (`Vm::install_ahead_body`),
//! and binds again when the VM's call of the site resolves another (`Vm::rebind_ahead_call`): the
//! guard's miss is the change, and the rows call the new closure by its entry from then on. A site
//! binds at most `POLYMORPHISM` closures, a site's feedback width; past it, the VM's call keeps it.
//!
//! Every closure bound stays alive and every descriptor and callee record written stays allocated for the image's
//! life: a record's identity is never a collected object's reused id, and a frame that names an
//! earlier descriptor (a callee running when its site rebinds) keeps reading a live one.
//!
//! Owner: one pipeline.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{AotImage, CallSite, unbound_callee_record};
use quench_runtime::folded_roots::RootedCode;
use crate::compiled::{tier_policy::POLYMORPHISM, DirectCallee};
use quench_bjit::call_record::{CalleeRecord, CallRecord};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::rc::Rc;

pub(crate) use quench_runtime::vm::engine_types::BoundCalls;

use quench_runtime::vm::engine_types::BoundRecords as Records;

use quench_runtime::vm::engine_types::Held;



#[quench_macros::ext(BoundCallsOps1)]
impl BoundCalls {
    /// The records of `image`, each its template.
    pub(crate) fn of(image: &AotImage) -> Self {
        let records = Records::Owned(image.calls.iter().map(|call| UnsafeCell::new(template(call))).collect());
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
        records.iter().zip(&image.calls).for_each(|(record, call)| unsafe { *record.get() = template(call) });
        Self::with_records(image, Records::Linked(records))
    }

    fn with_records(image: &AotImage, records: Records) -> Self {
        let mut sites: Vec<((u32, u32, u32), usize)> = image.calls.iter().enumerate().map(|(key, call)| ((call.function, call.pc, call.callee), key)).collect();
        sites.sort_unstable();
        let descriptors = image.calls.iter().map(|_| RefCell::default()).collect();
        let held = Held { callees: RefCell::default(), descriptors, records: RefCell::default() };
        Self { records, sites: sites.into(), held: Rc::new(held), rooted: Cell::new(false) }
    }

    pub(crate) fn records(&self) -> &[UnsafeCell<CallRecord>] {
        match &self.records {
            Records::Owned(records) => records,
            Records::Linked(records) => records,
        }
    }

    /// The key of the record of the call at `pc` of `function` to function `callee`, if the image proves that call.
    pub(crate) fn key(&self, site: (u32, u32, u32)) -> Option<usize> {
        self.sites.binary_search_by_key(&site, |(at, _)| *at).ok().map(|index| self.sites[index].1)
    }

    /// The callee record record `key` names now (`CallRecord::callee_record`): the unbound one until a binding.
    pub(crate) fn callee_record(&self, key: usize) -> Option<u64> {
        // SAFETY: the record is read on this thread, between runs of the code that reads it.
        self.records().get(key).map(|record| unsafe { (*record.get()).callee_record })
    }

    /// Whether record `key` names `callee`'s closure now.
    pub(crate) fn names(&self, key: usize, callee: quench_runtime::Value) -> bool {
        let Some(address) = self.callee_record(key) else { return false };
        // SAFETY: a callee record is the loader's unbound one or a binding's, alive for the image's life; read on this thread.
        unsafe { (*(address as *const CalleeRecord)).callee } == callee.to_bits()
    }

    /// Takes the current validity of record `key`'s callee cell where the callee record holds an older one: a cell's
    /// validity moves when its function's body changes (an image body installs at the first entry, after the
    /// loader bound the record), and a record that still names the callee but not its validity sends every call
    /// to the VM's. Whether it was stale.
    pub(crate) fn refresh(&self, key: usize) -> bool {
        let Some(address) = self.callee_record(key) else { return false };
        let callee = address as *mut CalleeRecord;
        // SAFETY: as `bind`: the callee record is the image's or a binding's, read by no code while the VM works on it;
        // a bound record's cell is its callee's live entry cell.
        unsafe {
            if (*callee).cell == 0 { return false; }
            let validity = ((*callee).cell as usize + quench_runtime::staged::call::ENTRY_CELL.validity_offset as usize) as *const u64;
            let now = validity.read();
            if (*callee).expected == now { return false; }
            (*callee).expected = now;
        }
        true
    }

    /// Binds record `key` to `direct`, the callee its site resolved. A new callee record holds the callee's words, as a
    /// JIT plan's callee record does (`DirectFacts::of`): the identity, the cell and the validity the call was planned
    /// under, the captures and global, the function, the descriptor (the callee's `DirectCallee` address, the native
    /// header's descriptor word), the context words, the window and the sloppiness. The site keeps its own words (the
    /// caller's call record and base), and names the new record last, which admits the rows and the AOT lane body's
    /// native call (`call_frames::push_native`). `None` where the site has bound its `POLYMORPHISM` closures already.
    pub(crate) fn bind(&self, key: usize, direct: DirectCallee, vm: &mut quench_runtime::vm::Vm) -> Option<()> {
        let (record, mut held) = (self.records().get(key)?.get(), self.held.descriptors.get(key)?.borrow_mut());
        if held.len() >= POLYMORPHISM { return None; }
        if !self.rooted.replace(true) {
            let code: Rc<dyn RootedCode> = self.held.clone();
            vm.hold_folded_roots(&code);
        }
        let identity = self.held.callees.borrow_mut().embed_heap_identity(direct.callee);
        held.push(Box::new(direct));
        // The box's contents do not move when the vector grows, so the descriptor address stays valid.
        let direct: &DirectCallee = &**held.last()?;
        let validity = (direct.cell + quench_runtime::staged::call::ENTRY_CELL.validity_offset as usize) as *const u64;
        let descriptor = std::ptr::from_ref(direct) as u64;
        let callee = Box::new(UnsafeCell::new(CalleeRecord {
            callee: identity, cell: direct.cell as u64, expected: validity.read(), captures: direct.capture_word(),
            global_this: direct.sloppy.map_or(0, quench_runtime::Value::to_bits), function: u64::from(direct.target.function),
            descriptor, env: direct.target.context_words().0, capture: direct.target.context_words().1,
            registers: u64::from(direct.registers), sloppy: u64::from(direct.sloppy.is_some()) }));
        let address = callee.get() as u64;
        self.held.records.borrow_mut().push(callee);
        // SAFETY: the record is the image's, read in place by code on this thread, none of which runs while the VM
        // binds; the callee record is the one just made, which no code reads until the site names it.
        unsafe {
            (*record).call = direct.call as u64;
            (*record).caller_base = direct.caller_base as u64;
            (*record).callee_record = address;
        }
        Some(())
    }
}

/// The template a record starts as: the image's words, naming no callee record until the loader sets it
/// (`unbound_callee_record`).
fn template(call: &CallSite) -> CallRecord {
    CallRecord { callee_record: unbound_callee_record(), ..call.record }
}
