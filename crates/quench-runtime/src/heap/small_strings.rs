//! Small strings: strings of parts (two or three, at most one a Number) in a 16-byte record
//! of their own space, where a string of parts (`JsString::from_parts`) takes a 24-byte string
//! record. Their ids are string ids of the hole the string record space keeps
//! (`records::STRING_RECORD_HOLE`), so a Value names one as it names any string and stays
//! narrow. The text is made on first read and kept beside the record (`SmallTexts`), owned by
//! it: the record's Values cannot be redirected, so the text is reached through it.
use super::*;
use super::records::{ParallelRecordMarks, STRING_RECORD_CAPACITY, STRING_RECORD_HOLE};
use crate::vm::wtf16::Part;
use crate::vm::wtf16::Units;
use std::cell::UnsafeCell;
use std::ptr::NonNull;

/// The small string records, by local id (a string id less `STRING_RECORD_HOLE`).
pub(super) type SmallStringSpace = records::RecordSpace<SmallParts, STRING_RECORD_HOLE>;

/// The bits of `SmallParts::head` and `tail` that hold a string id.
const ID_MASK: u32 = (1 << 28) - 1;
/// No string part in that place.
const NO_TEXT: u32 = ID_MASK;
/// Where `SmallParts::head` keeps its shape, one of the `SHAPE_*`.
const SHAPE_SHIFT: u32 = 28;
/// `[head] Number [tail]`, either string absent (`NO_TEXT`); `extra` holds the Number's bits.
const SHAPE_NUMBER_MIDDLE: u32 = 0;
/// `head tail Number`.
const SHAPE_NUMBER_LAST: u32 = 1;
/// `Number head tail`.
const SHAPE_NUMBER_FIRST: u32 = 2;
/// `head tail [extra]`, strings only; `extra` holds the third id, or `NO_TEXT`.
const SHAPE_TEXTS: u32 = 3;
/// `SmallParts::head` once the text has been made: `tail` names it in `SmallTexts`.
const WRITTEN: u32 = 1 << 31;
const _: () = assert!(STRING_RECORD_CAPACITY as u32 <= NO_TEXT && SHAPE_TEXTS << SHAPE_SHIFT < WRITTEN);

/// The parts of a string of parts (`Parts`: two or three, at most one a Number) in 16 bytes:
/// string ids in `head` (below its shape) and `tail`, and the Number's bits or a third id in
/// `extra`. Once read, `head` is `WRITTEN` and `tail` names its text.
#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct SmallParts {
    head: u32,
    tail: u32,
    extra: u64,
}
const _: () = assert!(size_of::<SmallParts>() == 16);

impl SmallParts {
    /// The record of the parts of a `Parts` string: its string ids in order, the first `texts`
    /// of `ids`, and its Number with the Number's position, if it has one; `None` when an id
    /// cannot be held.
    #[inline(always)]
    pub(crate) fn new(ids: [u32; 3], texts: usize, number: Option<(f64, usize)>) -> Option<Self> {
        debug_assert!((2..=3).contains(&(texts + usize::from(number.is_some()))));
        let id = |index: usize| if index < texts { ids[index] } else { NO_TEXT };
        if (0..texts).any(|index| ids[index] >= NO_TEXT) {
            return None;
        }
        let Some((number, at)) = number else {
            return Some(Self { head: SHAPE_TEXTS << SHAPE_SHIFT | id(0), tail: id(1), extra: u64::from(id(2)) });
        };
        let (shape, head, tail) = match (at, texts) {
            (0, 2) => (SHAPE_NUMBER_FIRST, id(0), id(1)),
            (0, _) => (SHAPE_NUMBER_MIDDLE, NO_TEXT, id(0)),
            (2, _) => (SHAPE_NUMBER_LAST, id(0), id(1)),
            _ => (SHAPE_NUMBER_MIDDLE, id(0), id(1)),
        };
        Some(Self { head: shape << SHAPE_SHIFT | head, tail, extra: number.to_bits() })
    }

    /// A record whose text, `text`, is made: as a snapshot restores it.
    pub(super) fn written(texts: &mut SharedSmallTexts, text: JsString) -> Self {
        Self { head: WRITTEN, tail: texts.get_mut().insert(text), extra: 0 }
    }

    /// Where its text is kept, once made.
    #[inline(always)]
    fn text_slot(&self) -> Option<u32> {
        (self.head == WRITTEN).then_some(self.tail)
    }

    /// Its parts in order, each a string id (`NO_TEXT` for none) or a Number; none once written.
    #[inline(always)]
    fn each(&self) -> [Option<Part>; 3] {
        let text = |id: u32| (id != NO_TEXT).then_some(Part::Text(id));
        let (head, tail) = (text(self.head & ID_MASK), text(self.tail));
        let number = Some(Part::Number(f64::from_bits(self.extra)));
        match self.head >> SHAPE_SHIFT {
            SHAPE_NUMBER_MIDDLE => [head, number, tail],
            SHAPE_NUMBER_LAST => [head, tail, number],
            SHAPE_NUMBER_FIRST => [number, head, tail],
            SHAPE_TEXTS => [head, tail, text(self.extra as u32)],
            _ => [None; 3],
        }
    }

    /// The string parts not yet written out: what a collector traces. Every shape keeps its
    /// ids in `head` and `tail`, and only `SHAPE_TEXTS` a third in `extra`.
    #[inline(always)]
    fn for_each_text(&self, mut visit: impl FnMut(u32)) {
        if self.head == WRITTEN {
            return;
        }
        let head = self.head & ID_MASK;
        if head != NO_TEXT {
            visit(head);
        }
        if self.tail != NO_TEXT {
            visit(self.tail);
        }
        if self.head >> SHAPE_SHIFT == SHAPE_TEXTS && self.extra as u32 != NO_TEXT {
            visit(self.extra as u32);
        }
    }
}

/// A string id is small when its bits under this mask are `SMALL_STRING_ID_BASE`: native
/// readers, which read string records in place, leave at their miss for one.
pub(crate) const SMALL_STRING_ID_MASK: u32 = !(STRING_RECORD_HOLE as u32 - 1);
pub(crate) const SMALL_STRING_ID_BASE: u32 = STRING_RECORD_HOLE as u32;

/// The local id of the string `record_id` when it is small.
#[inline(always)]
pub(crate) const fn small_string_local(record_id: u32) -> Option<u32> {
    if record_id as usize & !(STRING_RECORD_HOLE - 1) == STRING_RECORD_HOLE {
        Some(record_id - STRING_RECORD_HOLE as u32)
    } else {
        None
    }
}

/// Whether the string `record_id` is small: a string id no string record has.
#[inline(always)]
pub(crate) const fn is_small_string(record_id: u32) -> bool {
    small_string_local(record_id).is_some()
}

/// The string Value of the small string `local`.
pub(super) fn small_string_value(local: RecordId) -> Value {
    Value::direct_string(local + STRING_RECORD_HOLE as u32)
}

/// Texts per chunk of `SmallTexts`.
const TEXT_CHUNK: usize = 1024;
type TextChunk = [Option<JsString>; TEXT_CHUNK];

/// The texts of the small strings read so far, each owned by its record: made by a read
/// (through a shared heap), dropped by the sweep that frees the record. Chunks never move, so a
/// text read stays where it is while its record lives.
#[derive(Default)]
pub(super) struct SmallTexts {
    chunks: Vec<NonNull<TextChunk>>,
    free: Vec<u32>,
    next: u32,
}

impl SmallTexts {
    fn slot(&self, slot: u32) -> *mut Option<JsString> {
        let chunk = self.chunks[slot as usize / TEXT_CHUNK];
        // SAFETY: the chunk is live and the index is within it.
        unsafe { chunk.as_ptr().cast::<Option<JsString>>().add(slot as usize % TEXT_CHUNK) }
    }

    fn insert(&mut self, text: JsString) -> u32 {
        let slot = self.free.pop().unwrap_or_else(|| {
            let slot = self.next;
            if slot as usize == self.chunks.len() * TEXT_CHUNK {
                let chunk: Box<TextChunk> = Box::new([const { None }; TEXT_CHUNK]);
                self.chunks.push(NonNull::from(Box::leak(chunk)));
            }
            self.next += 1;
            slot
        });
        // SAFETY: a free or fresh slot holds `None`, and nothing refers to it.
        unsafe { *self.slot(slot) = Some(text) };
        slot
    }

    /// The text at `slot`.
    ///
    /// # Safety
    /// `slot` names a text a live record owns.
    unsafe fn get(&self, slot: u32) -> &JsString {
        // SAFETY: the caller's contract.
        unsafe { (*self.slot(slot)).as_ref().expect("a small string's text is kept") }
    }

    fn remove(&mut self, slot: u32) -> JsString {
        self.free.push(slot);
        // SAFETY: the slot's record was just freed, so nothing refers to its text.
        unsafe { (*self.slot(slot)).take().expect("a small string's text is kept") }
    }

    /// Bytes its chunks and free list take.
    pub(super) fn allocated_bytes(&self) -> usize {
        self.chunks.len() * size_of::<TextChunk>() + self.free.capacity() * size_of::<u32>()
    }
}

impl Drop for SmallTexts {
    fn drop(&mut self) {
        for chunk in self.chunks.drain(..) {
            // SAFETY: each chunk came from `Box::leak` and is dropped once.
            drop(unsafe { Box::from_raw(chunk.as_ptr()) });
        }
    }
}

/// `SmallTexts` behind a shared heap: a read makes a text through `&Heap`.
#[derive(Default)]
pub(super) struct SharedSmallTexts(UnsafeCell<SmallTexts>);

impl SharedSmallTexts {
    pub(super) fn get_mut(&mut self) -> &mut SmallTexts {
        self.0.get_mut()
    }

    fn get(&self) -> &SmallTexts {
        // SAFETY: the texts change only through `insert_shared`, which forms no lasting reference.
        unsafe { &*self.0.get() }
    }

    /// Keeps `text`, through a shared heap.
    fn insert_shared(&self, text: JsString) -> u32 {
        // SAFETY: the heap is single-threaded and no `&mut SmallTexts` is live while it is
        // shared; texts already handed out sit in chunks this does not move or write.
        unsafe { (*self.0.get()).insert(text) }
    }
}

/// Marks the string part `part` of a small string where the trace finds it, as `trace_parts`
/// does; any other part goes to `work`.
#[inline(always)]
fn mark_part(mark_flat: impl FnOnce(RecordId) -> bool, part: u32, work: &mut Vec<Value>) {
    if cfg!(feature = "profile-aggregate") || !mark_flat(part) {
        work.push(Value::direct_string(part));
    }
}

/// The parts the just-marked small string `local` holds, marked or pushed onto `work`.
pub(super) fn trace_small(small: &SmallStringSpace, records: &mut records::StringRecordSpace, local: RecordId, work: &mut Vec<Value>) {
    // SAFETY: the caller just marked the record, so it is occupied.
    let parts = unsafe { small.get_live(local) };
    parts.for_each_text(|part| mark_part(|id| string_parts::mark_flat(records, id), part, work));
}

/// `trace_small` through a parallel view.
pub(super) fn trace_small_if_new(
    small: &ParallelRecordMarks<'_, SmallParts>,
    records: &ParallelRecordMarks<'_, JsString>,
    local: RecordId,
    work: &mut Vec<Value>,
) {
    // SAFETY: the caller's view just marked the occupied record.
    let parts = unsafe { small.get(local) };
    parts.for_each_text(|part| mark_part(|id| string_parts::mark_flat_if_new(records, id), part, work));
}

/// Marks the small string `local` that an object property holds where the trace finds it,
/// and traces its parts as its visit would (`string_parts::mark_leaf_string_if_new`).
#[inline(always)]
pub(super) fn mark_leaf_small_if_new(
    small: &ParallelRecordMarks<'_, SmallParts>,
    records: &ParallelRecordMarks<'_, JsString>,
    local: RecordId,
    work: &mut Vec<Value>,
) -> bool {
    if cfg!(feature = "profile-aggregate") {
        return false;
    }
    if small.mark_if_new(local) {
        trace_small_if_new(small, records, local, work);
    }
    true
}

impl Heap {
    /// Bytes the small strings' records and texts take, past the texts' own storage.
    pub(super) fn small_string_bytes(&self) -> usize {
        self.small_strings.allocated_bytes() + self.small_texts.get().allocated_bytes()
    }

    /// A small string of `parts`; `None` when the space is full.
    #[inline(always)]
    pub(crate) fn alloc_small_string(&mut self, parts: SmallParts) -> Option<Value> {
        let local = self.small_strings.try_insert(parts).ok()?;
        self.allocated += 1;
        Some(small_string_value(local))
    }

    /// The text of the small string `local`, made first if it has none.
    ///
    /// # Safety
    /// `local` names a live small string, retained by a reachable Value.
    #[inline(always)]
    pub(super) unsafe fn small_string_text(&self, local: RecordId) -> &JsString {
        // SAFETY: the caller's contract.
        let parts = *unsafe { self.small_strings.get_live(local) };
        if let Some(slot) = parts.text_slot() {
            // SAFETY: the live record owns its text.
            return unsafe { self.small_texts.get().get(slot) };
        }
        // SAFETY: as above.
        unsafe { self.write_out_small_string(local, parts) }
    }

    /// The text of the small string record `parts`, which has one.
    ///
    /// # Safety
    /// `parts` is a live record of this heap.
    pub(super) unsafe fn small_strings_text_of(&self, parts: &SmallParts) -> &JsString {
        let slot = parts.text_slot().expect("the small string was written out");
        // SAFETY: the caller's contract: a live record owns its text.
        unsafe { self.small_texts.get().get(slot) }
    }

    #[cold]
    #[inline(never)]
    unsafe fn write_out_small_string(&self, local: RecordId, parts: SmallParts) -> &JsString {
        let mut number = crate::number_to_string::Short::new();
        let pieces = parts.each().map(|part| match part {
            None => &[][..],
            // SAFETY: a part is a string its small string keeps alive.
            Some(Part::Text(id)) => match unsafe { self.written_out_string(id) }.units() {
                Units::Ascii(bytes) => bytes,
                Units::Wide(_) => unreachable!("a part is ASCII (`Vm::string_of_parts`)"),
            },
            Some(Part::Number(value)) => {
                crate::number_to_string::write(value, &mut number);
                b""
            }
        });
        let length = pieces.iter().map(|piece| piece.len()).sum::<usize>() + number.len();
        let at = parts.each().iter().position(|part| matches!(part, Some(Part::Number(_))));
        let joined = JsString::from_ascii_parts(length, pieces.iter().enumerate().map(|(position, piece)| {
            if Some(position) == at { number.as_slice() } else { piece }
        }));
        let slot = self.small_texts.insert_shared(joined);
        let record = self.small_strings.base_ptr().expect("a live record has a space").cast_mut();
        // SAFETY: the record is live and initialized, and no reference to it outlives a read
        // (each copies it); the collector does not run now. The text it now names is owned and
        // holds no edge, so an old record needs no barrier.
        unsafe {
            let record = record.add(local as usize);
            (*record).head = WRITTEN;
            (*record).tail = slot;
            self.small_texts.get().get(slot)
        }
    }
}

/// Sweeps the small strings as the string records are swept: a dead one's text is dropped and
/// released from the payload ledger; a full sweep counts each live text's.
pub(super) fn sweep(
    small: &mut SmallStringSpace,
    texts: &mut SharedSmallTexts,
    payload_bytes: &mut usize,
    live_payload_bytes: &mut usize,
    full: bool,
    age: PromotionAge,
) -> (usize, usize) {
    let texts = texts.get_mut();
    small.sweep(full, age, full, |record| match record {
        RecordSweep::Keep(_, parts) => {
            if let Some(slot) = parts.text_slot() {
                // SAFETY: a kept record owns its text.
                *live_payload_bytes += unsafe { texts.get(slot) }.flat_unit_bytes();
            }
        }
        RecordSweep::Dead(_, parts) => {
            if let Some(slot) = parts.text_slot() {
                *payload_bytes = payload_bytes.saturating_sub(texts.remove(slot).flat_unit_bytes());
            }
        }
    })
}
