//! Small strings: strings of parts (two or three, at most one a Number) in a 12-byte record
//! of their own space, where a string of parts (`JsString::from_parts`) takes a 24-byte string
//! record. Their ids are string ids of the hole the string record space keeps
//! (`records::STRING_RECORD_HOLE`), so a Value names one as it names any string and stays
//! narrow. The text is made on first read and replaces the parts in the record: its shared
//! storage's pointer and length, the 12 bytes still. A read is answered by a view of that text
//! (`SmallViews`), a string value held until the next collection, which no reference outlives.
use super::*;
use super::records::{ParallelRecordMarks, STRING_RECORD_CAPACITY, STRING_RECORD_HOLE};
use crate::vm::wtf16::Part;
use crate::vm::wtf16::Units;
use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use std::ptr::NonNull;
use std::rc::Rc;

/// The small string records, by local id (a string id less `STRING_RECORD_HOLE`).
pub(super) type SmallStringSpace = records::RecordSpace<SmallParts, STRING_RECORD_HOLE>;

/// The bits of `SmallParts::head` and `tail` that hold a string id.
const ID_MASK: u32 = (1 << 28) - 1;
/// No string part in that place.
const NO_TEXT: u32 = ID_MASK;
/// Where `SmallParts::head` keeps its shape, one of the `SHAPE_*`.
const SHAPE_SHIFT: u32 = 28;
/// `[head] Number [tail]`, either string absent (`NO_TEXT`); `extra` holds the Number, an int32.
const SHAPE_INT_MIDDLE: u32 = 0;
/// `head tail Number`, the Number an int32 in `extra`.
const SHAPE_INT_LAST: u32 = 1;
/// `Number head tail`, the Number an int32 in `extra`.
const SHAPE_INT_FIRST: u32 = 2;
/// `head tail [extra]`, strings only; `extra` holds the third id, or `NO_TEXT`.
const SHAPE_TEXTS: u32 = 3;
/// `Number head`: one string and a Number of any value, its bits in `tail` (low) and `extra`.
const SHAPE_WIDE_FIRST: u32 = 4;
/// `head Number`, as `SHAPE_WIDE_FIRST`.
const SHAPE_WIDE_LAST: u32 = 5;
/// `prefix Number suffix`: `head` holds a template (`SmallTemplates`), the pair of strings
/// around the Number, which may be any double, in `tail` (low) and `extra`.
const SHAPE_WIDE_MIDDLE: u32 = 6;
/// `SmallParts::head`'s flag once the text has been made: the bits below it hold the text's
/// length, and `tail` (low) with `extra` one word (`written_word`): the address `Rc::into_raw`
/// gave for its storage over `VIEW_BITS`, and below them the slot of its latest view
/// (`SmallViews`), which may be stale.
const WRITTEN: u32 = 1 << 31;
/// Bits of the written word holding the view slot.
const VIEW_BITS: u32 = 19;
const VIEW_MASK: u64 = (1 << VIEW_BITS) - 1;
/// No view yet, or a slot the word cannot hold.
const NO_VIEW: u32 = VIEW_MASK as u32;
/// Low bits a text storage address always has clear (its `Rc` is word aligned).
const TEXT_ALIGN_BITS: u32 = 3;
const _: () = assert!(STRING_RECORD_CAPACITY as u32 <= NO_TEXT && SHAPE_WIDE_MIDDLE << SHAPE_SHIFT < WRITTEN);
const _: () = assert!(VIEW_COLLECTION_LIMIT < NO_VIEW && 48 - TEXT_ALIGN_BITS + VIEW_BITS <= 64);

/// The parts of a string of parts (`Parts`: two or three, at most one a Number) in 12 bytes:
/// string ids in `head` (below its shape) and `tail`, and a third id or an int32 Number in
/// `extra`; a Number beside one string may be any double, in `tail` and `extra`. Once read, the
/// record owns its text instead (`WRITTEN`).
#[repr(C)]
pub struct SmallParts {
    head: u32,
    tail: u32,
    extra: u32,
}
const _: () = assert!(size_of::<SmallParts>() == 12);

/// A Number's int32 when it is exactly one (not -0).
#[inline(always)]
fn exact_int(number: f64) -> Option<i32> {
    let int = number as i32;
    (f64::from(int).to_bits() == number.to_bits()).then_some(int)
}

impl SmallParts {
    /// The record of the parts of a `Parts` string: its string ids in order, the first `texts`
    /// of `ids`, and its Number with the Number's position, if it has one; `None` when an id
    /// cannot be held, or a Number beside two strings is not an int32.
    #[inline(always)]
    pub(crate) fn new(ids: [u32; 3], texts: usize, number: Option<(f64, usize)>) -> Option<Self> {
        debug_assert!((2..=3).contains(&(texts + usize::from(number.is_some()))));
        let id = |index: usize| if index < texts { ids[index] } else { NO_TEXT };
        if (0..texts).any(|index| ids[index] >= NO_TEXT) {
            return None;
        }
        let Some((number, at)) = number else {
            return Some(Self { head: SHAPE_TEXTS << SHAPE_SHIFT | id(0), tail: id(1), extra: id(2) });
        };
        let Some(int) = exact_int(number) else {
            if texts != 1 {
                return None;
            }
            let shape = if at == 0 { SHAPE_WIDE_FIRST } else { SHAPE_WIDE_LAST };
            let bits = number.to_bits();
            return Some(Self { head: shape << SHAPE_SHIFT | id(0), tail: bits as u32, extra: (bits >> 32) as u32 });
        };
        let (shape, head, tail) = match (at, texts) {
            (0, 2) => (SHAPE_INT_FIRST, id(0), id(1)),
            (0, _) => (SHAPE_INT_MIDDLE, NO_TEXT, id(0)),
            (2, _) => (SHAPE_INT_LAST, id(0), id(1)),
            _ => (SHAPE_INT_MIDDLE, id(0), id(1)),
        };
        Some(Self { head: shape << SHAPE_SHIFT | head, tail, extra: int as u32 })
    }

    /// The record of `prefix Number suffix` whose strings are the pair `template`
    /// (`Heap::small_template`).
    #[inline(always)]
    pub(crate) fn templated(template: u32, number: f64) -> Self {
        debug_assert!(template < NO_TEXT);
        let bits = number.to_bits();
        Self { head: SHAPE_WIDE_MIDDLE << SHAPE_SHIFT | template, tail: bits as u32, extra: (bits >> 32) as u32 }
    }

    /// A record that owns `text`, already made (`WRITTEN`).
    pub(super) fn written(text: JsString) -> Self {
        let text = text.into_shared_ascii();
        let length = u32::try_from(text.len()).ok().filter(|length| length & WRITTEN == 0).expect("a short text");
        let address = Rc::into_raw(text).cast::<u8>() as u64;
        debug_assert!(address.trailing_zeros() >= TEXT_ALIGN_BITS && address >> 48 == 0);
        let word = address >> TEXT_ALIGN_BITS << VIEW_BITS | u64::from(NO_VIEW);
        Self { head: WRITTEN | length, tail: word as u32, extra: (word >> 32) as u32 }
    }

    /// `tail` and `extra` as one word: a wide Number's bits, or a written text's address and view.
    #[inline(always)]
    fn word(&self) -> u64 {
        u64::from(self.tail) | u64::from(self.extra) << 32
    }

    /// The slot of a written record's latest view, possibly stale (`NO_VIEW` for none).
    #[inline(always)]
    fn view_slot(&self) -> u32 {
        (self.word() & VIEW_MASK) as u32
    }

    /// Whether its text has been made.
    #[inline(always)]
    fn is_written(&self) -> bool {
        self.head & WRITTEN != 0
    }

    /// Whether dropping it writes no reference count: it holds no text yet (a written
    /// text's `Rc` may have been cloned out through a view).
    pub(super) fn drop_is_local(&self) -> bool {
        !self.is_written()
    }

    /// Its text's storage, once made, still owned by the record.
    #[inline(always)]
    fn text(&self) -> Option<ManuallyDrop<Rc<str>>> {
        self.is_written().then(|| {
            let address = (self.word() >> VIEW_BITS) << TEXT_ALIGN_BITS;
            let bytes = std::ptr::slice_from_raw_parts(address as *const u8, (self.head & !WRITTEN) as usize);
            // SAFETY: `written` kept this storage's `Rc::into_raw`, and the record still owns it.
            ManuallyDrop::new(unsafe { Rc::from_raw(bytes as *const str) })
        })
    }

    /// A string of its text, which it has; for a snapshot.
    pub(super) fn written_text(&self) -> JsString {
        JsString::from_shared_text(&self.text().expect("the small string was written out"))
    }

    /// Bytes of its text, once made: what the payload ledger counts.
    pub(super) fn text_bytes(&self) -> usize {
        if self.is_written() { (self.head & !WRITTEN) as usize } else { 0 }
    }

    /// Its parts in order, each a string id (`NO_TEXT` for none) or a Number; none once written.
    #[inline(always)]
    fn each(&self, templates: &SmallTemplates) -> [Option<Part>; 3] {
        let text = |id: u32| (id != NO_TEXT).then_some(Part::Text(id));
        let (head, tail) = (text(self.head & ID_MASK), text(self.tail));
        let int = Some(Part::Number(f64::from(self.extra as i32)));
        let wide = Some(Part::Number(f64::from_bits(self.word())));
        match self.head >> SHAPE_SHIFT {
            SHAPE_INT_MIDDLE => [head, int, tail],
            SHAPE_INT_LAST => [head, tail, int],
            SHAPE_INT_FIRST => [int, head, tail],
            SHAPE_TEXTS => [head, tail, text(self.extra)],
            SHAPE_WIDE_FIRST => [wide, head, None],
            SHAPE_WIDE_LAST => [head, wide, None],
            SHAPE_WIDE_MIDDLE => {
                let (prefix, suffix) = templates.pairs[(self.head & ID_MASK) as usize];
                [Some(Part::Text(prefix)), wide, Some(Part::Text(suffix))]
            }
            _ => [None; 3],
        }
    }

    /// The string parts not yet written out: what a collector traces. Every shape but
    /// `SHAPE_WIDE_MIDDLE`, whose template's strings stay rooted (`SmallTemplates`), keeps an id
    /// in `head`; all but the wide shapes one in `tail`, and only `SHAPE_TEXTS` a third in `extra`.
    #[inline(always)]
    pub(super) fn for_each_text(&self, mut visit: impl FnMut(u32)) {
        if self.is_written() {
            return;
        }
        let shape = self.head >> SHAPE_SHIFT;
        let head = self.head & ID_MASK;
        if head != NO_TEXT && shape != SHAPE_WIDE_MIDDLE {
            visit(head);
        }
        if shape <= SHAPE_TEXTS && self.tail != NO_TEXT {
            visit(self.tail);
        }
        if shape == SHAPE_TEXTS && self.extra != NO_TEXT {
            visit(self.extra);
        }
    }
}


/// A string's existing flat units (`Heap::flat_text`): borrowed from its record, or a small
/// string's shared text.
pub enum FlatText<'a> {
    Record(Units<'a>),
    Small(Rc<str>),
}

impl FlatText<'_> {
    #[inline(always)]
    pub(crate) fn units(&self) -> Units<'_> {
        match self {
            Self::Record(units) => *units,
            Self::Small(text) => Units::Ascii(text.as_bytes()),
        }
    }
}

impl Drop for SmallParts {
    fn drop(&mut self) {
        if let Some(text) = self.text() {
            drop(ManuallyDrop::into_inner(text));
        }
    }
}

/// A string id is small when its bits under this mask are `SMALL_STRING_ID_BASE`: native
/// readers, which read string records in place, leave at their miss for one.
pub use crate::staged::layout_offsets::{SMALL_STRING_ID_BASE, SMALL_STRING_ID_MASK};

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
pub const fn is_small_string(record_id: u32) -> bool {
    small_string_local(record_id).is_some()
}

/// The string Value of the small string `local`.
pub(super) fn small_string_value(local: RecordId) -> Value {
    Value::direct_string(local + STRING_RECORD_HOLE as u32)
}

/// Views per chunk of `SmallViews`.
const VIEW_CHUNK: usize = 1024;
/// A view and the local id of the small string it shows.
type ViewChunk = [(u32, Option<JsString>); VIEW_CHUNK];
/// Views made since the last collection past which the next safepoint collects, which drops
/// them: about 2 MiB of views at most.
const VIEW_COLLECTION_LIMIT: u32 = 1 << 16;
/// Chunks a collection keeps for the views after it.
const KEPT_VIEW_CHUNKS: usize = 1;

/// The strings that answer reads of written small strings since the last collection, each a
/// view sharing its record's text storage. A reference to one lives no longer than the shared
/// borrow of the heap it came from, and every collection takes the heap mutably first, so a
/// collection drops them all (`clear`); a record names its latest by slot, checked by owner.
/// Chunks never move while views are handed out.
#[derive(Default)]
pub(super) struct SmallViewChunks {
    chunks: Vec<NonNull<ViewChunk>>,
    len: u32,
}

impl SmallViewChunks {
    fn entry(&self, slot: u32) -> *mut (u32, Option<JsString>) {
        let chunk = self.chunks[slot as usize / VIEW_CHUNK];
        // SAFETY: the chunk is live and the index is within it.
        unsafe { chunk.as_ptr().cast::<(u32, Option<JsString>)>().add(slot as usize % VIEW_CHUNK) }
    }

    /// The view at `slot` when it shows the small string `owner`.
    fn get(&self, slot: u32, owner: u32) -> Option<&JsString> {
        if slot >= self.len {
            return None;
        }
        // SAFETY: slots below `len` hold views, none of which is being written.
        let (shown, view) = unsafe { &*self.entry(slot) };
        (*shown == owner).then(|| view.as_ref().expect("a slot below the length holds a view"))
    }

    fn push(&mut self, owner: u32, view: JsString) -> u32 {
        let slot = self.len;
        if slot as usize == self.chunks.len() * VIEW_CHUNK {
            let chunk: Box<ViewChunk> = Box::new([const { (0, None) }; VIEW_CHUNK]);
            self.chunks.push(NonNull::from(Box::leak(chunk)));
        }
        // SAFETY: the slot at `len` holds no view, and nothing refers to it.
        unsafe { *self.entry(slot) = (owner, Some(view)) };
        self.len += 1;
        slot
    }

    /// Drops every view; keeps `KEPT_VIEW_CHUNKS` chunks for the next.
    fn clear(&mut self) {
        for slot in 0..self.len {
            // SAFETY: the caller holds the heap mutably, so no view is referred to.
            unsafe { (*self.entry(slot)).1 = None };
        }
        self.len = 0;
        for chunk in self.chunks.drain(KEPT_VIEW_CHUNKS.min(self.chunks.len())..) {
            // SAFETY: each chunk came from `Box::leak`, holds no view now, and is freed once.
            drop(unsafe { Box::from_raw(chunk.as_ptr()) });
        }
    }

    /// Bytes its chunks take.
    pub(super) fn allocated_bytes(&self) -> usize {
        self.chunks.len() * size_of::<ViewChunk>()
    }
}

impl Drop for SmallViewChunks {
    fn drop(&mut self) {
        self.clear();
        for chunk in self.chunks.drain(..) {
            // SAFETY: as in `clear`.
            drop(unsafe { Box::from_raw(chunk.as_ptr()) });
        }
    }
}

/// `SmallViewChunks` behind a shared heap: a read makes a view through `&Heap`.
#[derive(Default)]
pub(super) struct SmallViews(UnsafeCell<SmallViewChunks>);

impl SmallViews {
    /// Drops every view: at a collection, which holds the heap mutably.
    pub(super) fn clear(&mut self) {
        self.0.get_mut().clear();
    }

    #[inline]
    fn get(&self) -> &SmallViewChunks {
        // SAFETY: the views change only through `push_shared`, which forms no lasting reference.
        unsafe { &*self.0.get() }
    }

    /// Keeps `view` of the small string `owner`, through a shared heap.
    fn push_shared(&self, owner: u32, view: JsString) -> u32 {
        // SAFETY: the heap is single-threaded and no `&mut SmallViewChunks` is live while it is
        // shared; views already handed out sit in chunks this does not move or write.
        unsafe { (*self.0.get()).push(owner, view) }
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

/// Templates: pairs of strings that a Number joins (`SHAPE_WIDE_MIDDLE`), so `"a" + x + "b"`
/// with any Number `x` takes a 12-byte small string. Each pair is interned once; its strings
/// are rooted for the heap's life, which the bounds on the count and on each part's length
/// (`Vm::string_of_parts`) keep to a few hundred kilobytes at most.
#[derive(Default)]
pub(crate) struct SmallTemplates {
    pairs: Vec<(u32, u32)>,
    index: rustc_hash::FxHashMap<(u32, u32), u32>,
}

/// Templates a heap interns; a pair past them takes a string-of-parts record.
const TEMPLATE_CAPACITY: usize = 4096;

impl Heap {
    /// The template of the strings `prefix` and `suffix` around a Number, interned now if it
    /// is new; `None` once the table is full.
    #[inline]
    pub(crate) fn small_template(&mut self, prefix: u32, suffix: u32) -> Option<u32> {
        if let Some(&template) = self.small_templates.index.get(&(prefix, suffix)) {
            return Some(template);
        }
        if self.small_templates.pairs.len() >= TEMPLATE_CAPACITY {
            return None;
        }
        // The pair's strings live as long as the table names them.
        self.roots.insert(Value::direct_string(prefix));
        self.roots.insert(Value::direct_string(suffix));
        let template = self.small_templates.pairs.len() as u32;
        self.small_templates.pairs.push((prefix, suffix));
        self.small_templates.index.insert((prefix, suffix), template);
        Some(template)
    }

    /// Bytes the small strings' records and views take, past their texts' own storage.
    pub(super) fn small_string_bytes(&self) -> usize {
        self.small_strings.allocated_bytes() + self.small_views.get().allocated_bytes()
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
        let record = unsafe { self.small_strings.get_live(local) };
        if record.is_written()
            && let Some(view) = self.small_views.get().get(record.view_slot(), local)
        {
            return view;
        }
        // SAFETY: as above.
        unsafe { self.view_small_string(local) }
    }

    /// Writes out the small string `local` if it is parts: its text replaces them.
    ///
    /// # Safety
    /// As `small_string_text`.
    #[inline(always)]
    unsafe fn write_out_small_string(&self, local: RecordId) -> &SmallParts {
        // SAFETY: the caller's contract.
        let record = unsafe { self.small_strings.get_live(local) };
        if !record.is_written() {
            // SAFETY: as above.
            unsafe { self.replace_parts_with_text(local) };
        }
        // SAFETY: as above.
        unsafe { self.small_strings.get_live(local) }
    }

    #[cold]
    #[inline(never)]
    unsafe fn replace_parts_with_text(&self, local: RecordId) {
        // SAFETY: the caller's contract.
        let text = self.small_parts_text(unsafe { self.small_strings.get_live(local) });
        let written = ManuallyDrop::new(SmallParts::written(text));
        let base = self.small_strings.base_ptr().expect("a live record has a space").cast_mut();
        // SAFETY: the record is live; its parts own nothing, so overwriting drops nothing, and
        // no reference to it is held across this (callers read it again after). The collector
        // does not run now. Its text holds no edge, so an old record needs no barrier; the
        // parts it no longer names are not traced through it from now on.
        unsafe { std::ptr::copy_nonoverlapping(&*written as *const SmallParts, base.add(local as usize), 1) };
    }

    /// Writes out the small string `local` if it is parts, and makes a view of its text.
    #[cold]
    #[inline(never)]
    unsafe fn view_small_string(&self, local: RecordId) -> &JsString {
        // SAFETY: the caller's contract.
        let record = unsafe { self.write_out_small_string(local) };
        let view = JsString::from_shared_text(&record.text().expect("written above"));
        crate::vm::wtf16::charge_materialized_beside(size_of::<(u32, Option<JsString>)>());
        let slot = self.small_views.push_shared(local, view);
        if slot + 1 >= VIEW_COLLECTION_LIMIT {
            // A collection drops the views; a loop reading many strings once would keep them all.
            self.request_collection();
        }
        let base = self.small_strings.base_ptr().expect("a live record has a space").cast_mut();
        // SAFETY: as above; the view bits of a written record are only a hint, read by copy.
        // A slot past the bits leaves `NO_VIEW`, which no view answers.
        let record = unsafe { &mut *base.add(local as usize) };
        record.tail = record.tail & !(VIEW_MASK as u32) | slot.min(NO_VIEW);
        self.small_views.get().get(slot, local).expect("the view just made")
    }

    /// `string(value)?.existing_flat()?.ascii_len()` with no view made of a small string.
    #[inline(always)]
    pub(crate) fn existing_ascii_len(&self, value: Value) -> Option<usize> {
        let record_id = value.direct_string_index()?;
        if let Some(local) = small_string_local(record_id) {
            // SAFETY: string Values come only from allocation and stay live while reachable.
            return Some(unsafe { self.write_out_small_string(local) }.text_bytes());
        }
        // SAFETY: as above.
        unsafe { self.string_record_by_id(record_id) }.existing_flat()?.ascii_len()
    }

    /// The existing flat units of the string `value`, as `string(value)?.existing_flat()?.view()`
    /// reads them, but with no view made of a small string: what a reader that only copies or
    /// measures the units takes.
    #[inline(always)]
    pub(crate) fn flat_text(&self, value: Value) -> Option<FlatText<'_>> {
        let record_id = value.direct_string_index()?;
        if let Some(local) = small_string_local(record_id) {
            // SAFETY: string Values come only from allocation and stay live while reachable.
            let record = unsafe { self.write_out_small_string(local) };
            return Some(FlatText::Small(Rc::clone(&record.text().expect("written above"))));
        }
        // SAFETY: as above.
        Some(FlatText::Record(unsafe { self.string_record_by_id(record_id) }.existing_flat()?.view()))
    }

    /// The text of the parts of `record`, each part written out first.
    fn small_parts_text(&self, record: &SmallParts) -> JsString {
        let mut number = crate::number_to_string::Short::new();
        let pieces = record.each(&self.small_templates).map(|part| match part {
            None => &[][..],
            // SAFETY: a part is a string its small string keeps alive.
            Some(Part::Text(id)) => match unsafe { self.written_out_string(id) }.units() {
                Units::Ascii(bytes) => bytes,
                Units::Latin1(_) | Units::Wide(_) => unreachable!("a part is ASCII (`Vm::string_of_parts`)"),
            },
            Some(Part::Number(value)) => {
                crate::number_to_string::write(value, &mut number);
                b""
            }
        });
        let length = pieces.iter().map(|piece| piece.len()).sum::<usize>() + number.len();
        let at = record.each(&self.small_templates).iter().position(|part| matches!(part, Some(Part::Number(_))));
        JsString::from_ascii_parts(length, pieces.iter().enumerate().map(|(position, piece)| {
            if Some(position) == at { number.as_slice() } else { piece }
        }))
    }
}
