//! Loop entry: the entry record of a loop region, its entry words and backedge budget, and the entry's methods.
use super::*;

/// The entry record native code reads: each capture's value, then each cached field's,
/// then each array's element pointer, length and capacity, then each guarded field-store
/// pointer. Only the words pushed are ever read, by native code and by `captures` and
/// `field_value`, so the rest stay uninitialized.
pub(crate) struct LoopEntry {
    words: [std::mem::MaybeUninit<u64>; ENTRY_WORDS],
    length: usize,
    /// An inlining region's callee registers, which leaving at an inlined instruction
    /// writes back here for its activation.
    scratch: [std::mem::MaybeUninit<u64>; MAX_REGION_REGISTERS],
    /// What the budget word was primed with for the current run (`set_backedge_budget`; `BACKEDGE_BUDGET` by default).
    primed: i64,
}

/// The longest array a region indexes: a negative `i32` index zero-extends past it, so one
/// unsigned comparison bounds every `i32` index.
pub(crate) const MAX_INDEXED_LENGTH: usize = i32::MAX as usize;

/// The first record word counts executions of the region's own backward edge: the iterations
/// a run completed natively, which weigh what its entry paid for (`LoopSite::note_run`).
pub(super) const ENTRY_BACKEDGE_COUNT: usize = 0;

/// The backward edges one native run may take before it leaves to the interpreter, which re-enters the loop (a run's
/// budget is the record's first word, primed here and debited by the stencils' `count_edge` and the lane's flush).
pub(crate) const BACKEDGE_BUDGET: i64 = 1 << 20;

pub(crate) const ENTRY_CAPTURES: usize = 1;

/// The entry word of the region's `index`th field: the single home of the record's field
/// layout, shared by the verified lowering and the lane region's snapshot reads.
pub(crate) fn field_word(region: &LoopRegion, index: usize) -> usize {
    ENTRY_CAPTURES + region.captures.len() + index
}

/// An array's entry words: its element pointer, its length, then its capacity (which only
/// a written array's appends read).
pub(crate) const ARRAY_WORDS: usize = 3;

pub(super) const FIELD_STORE_WORDS: usize = 1;

/// The heap views native reads use (`LoopEntry::push_native_heap`): the object records'
/// base, the property values' base and count, and the array records' base.
pub(super) const NATIVE_HEAP_WORDS: usize = 4;

pub(super) const ENTRY_WORDS: usize =
    ENTRY_CAPTURES
        + MAX_REGION_CAPTURES
        + ARRAY_WORDS * MAX_REGION_ARRAYS
        + MAX_REGION_FIELDS
        + FIELD_STORE_WORDS * MAX_REGION_FIELD_STORES
        + NATIVE_HEAP_WORDS;

impl LoopEntry {
    pub(crate) fn new() -> Self {
        let mut words = [std::mem::MaybeUninit::uninit(); ENTRY_WORDS];
        // The backward-edge budget is the one leading word native code reads before any push.
        words[ENTRY_BACKEDGE_COUNT].write(BACKEDGE_BUDGET as u64);
        Self { words, length: ENTRY_CAPTURES, scratch: [std::mem::MaybeUninit::uninit(); MAX_REGION_REGISTERS], primed: BACKEDGE_BUDGET }
    }

    /// Number of taken executions of this region's backward edge in the most recent run: the budget the word was
    /// primed with less what it has left.
    pub(crate) fn backedge_count(&self) -> u64 {
        // SAFETY: `new` initializes the budget word.
        let left = unsafe { self.words[ENTRY_BACKEDGE_COUNT].assume_init() } as i64;
        (self.primed - left).max(0) as u64
    }

    /// The record's words. Native OSR may also update the private first counter word.
    pub(crate) fn words_mut(&mut self) -> *mut u64 {
        debug_assert!(self.length <= ENTRY_WORDS);
        self.words.as_mut_ptr().cast()
    }

    pub(super) fn push(&mut self, word: u64) {
        self.words[self.length].write(word);
        self.length += 1;
    }

    /// Captures come first, in the region's order.
    pub(crate) fn push_capture(&mut self, value: Value) {
        self.push(value.to_bits());
    }

    /// The first `count` capture values, as pushed.
    pub(crate) fn captures(&self, count: usize) -> impl Iterator<Item = Value> + '_ {
        debug_assert!(ENTRY_CAPTURES + count <= self.length);
        self.words[ENTRY_CAPTURES..ENTRY_CAPTURES + count]
            .iter()
            // SAFETY: the first `count` words after the leading ones are the pushed captures.
            .map(|word| Value::from_bits(unsafe { word.assume_init() }))
    }

    /// Arrays follow the fields, in the region's order. An array longer than
    /// `MAX_INDEXED_LENGTH` declines the entry.
    pub(crate) fn push_array<T>(&mut self, elements: *mut T, length: usize, capacity: usize) -> Option<()> {
        (length <= MAX_INDEXED_LENGTH).then_some(())?;
        let end = self.length + ARRAY_WORDS;
        self.words[self.length..end].copy_from_slice(&[
            std::mem::MaybeUninit::new(elements.addr() as u64),
            std::mem::MaybeUninit::new(length as u64),
            std::mem::MaybeUninit::new(capacity.min(MAX_INDEXED_LENGTH) as u64),
        ]);
        self.length = end;
        Some(())
    }


    /// A warmed own-data field slot guarded by the VM during entry setup.
    pub(crate) fn push_field_store(&mut self, slot: *mut Value) {
        self.push(slot.addr() as u64);
    }

    /// Heap views for native field reads and array accesses (`NATIVE_HEAP_WORDS`). The
    /// pointers are valid only for this synchronous run and are never retained in compiled
    /// code.
    pub(crate) fn push_native_heap(
        &mut self,
        objects: *const quench_runtime::tier_surface::Object,
        properties: *const Value,
        property_len: usize,
        arrays: *const quench_runtime::tier_surface::ArrayObject,
    ) {
        self.push(objects.addr() as u64);
        self.push(properties.addr() as u64);
        self.push(property_len as u64);
        self.push(arrays.addr() as u64);
    }

    /// The value of the region's `index`th field as pushed: its word follows the captures
    /// and the fields before it.
    pub(crate) fn field_value(&self, region: &LoopRegion, index: usize) -> Value {
        let word = field_word(region, index);
        debug_assert!(word < self.length);
        // SAFETY: the caller pushed the field, after the captures and the fields before it.
        Value::from_bits(unsafe { self.words[word].assume_init() })
    }

    /// The scratch window an inlining region's leave inside an inlined callee writes the
    /// callee's live registers to, at each register's offset past the caller's
    /// (`JsStepDispatch::inlined`).
    /// The window's first `width` words start `undefined`, so a register the leave did not write
    /// reads as a value even if the callee's and the unit's liveness ever disagreed; only inlining
    /// regions ask for the window, and `scratch_value` reads no word past `Inlining::window`.
    pub(crate) fn scratch_address(&mut self, width: usize) -> usize {
        let filled = width.min(self.scratch.len());
        self.scratch[..filled].fill(std::mem::MaybeUninit::new(Value::UNDEFINED.to_bits()));
        self.scratch.as_mut_ptr().addr()
    }

    /// The value an inlined callee's register `register` of an inlining region took when
    /// native code left at an inlined instruction: `undefined` where it was not live.
    ///
    /// # Safety
    /// The exit wrote the register back: it is live at the callee's PC the exit names.
    pub(crate) unsafe fn scratch_value(&self, inlining: &Inlining, register: Register) -> Value {
        // SAFETY: as the caller promises.
        Value::from_bits(unsafe { self.scratch[usize::from(register - inlining.caller_registers)].assume_init() })
    }

    /// A field's value; a Number field's must be a Number.
    pub(crate) fn push_field(&mut self, value: Value, tagged: bool) -> Option<()> {
        (tagged || value.as_number().is_some()).then_some(())?;
        self.push(value.to_bits());
        Some(())
    }
}
