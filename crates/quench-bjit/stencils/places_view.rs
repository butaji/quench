//! The loop-entry view frame of an element row: `PlacesView` wraps a row's `Places` and answers the
//! element callouts (`ElementFrame::kept_view`) from the array view the loop's entry proved, which
//! the row read from its site hole. The element rows then run the staged `ElementRead` /
//! `ElementWrite` through that wrapper (`element_rows::step`), so an entry row and a dense row are
//! one definition: the entry view replaces the proof `prove_view` would make, and every other
//! answer (the key's int32, the store gate, the replacement) is `Places`' own.
//!
//! An entry row's array is not a frame slot of its row: the hole the frame would name holds the
//! entry's site offset (or another operand), so `PlacesView::tagged` answers the array register
//! with 0 and never loads it. Nothing reads that word: a kept view answers the access, and the
//! element rows' host path (`index_get_host`, `index_set_host`) is the trait default, which leaves.
//!
//! Holes: the dense entry read and store take the entry site at hole 1 (read) or 0 (store), the
//! typed rows the same site at hole 1 (read) or 0 (store), exactly as the hand rows read them.

use crate::bytecode::{Operand, Register};
use crate::heap_rows::{stage_site, word_at};
use crate::layout::{JS_STEP_LOOP_ENTRY, LOOP_ENTRY_ARRAY_COUNT_AT, TypedArrayKind};
use crate::typed_ir::{Representation, Type};
use crate::places::Places;
use crate::stage::StencilStage;
use crate::staged::element_rows::ElementView;
use crate::staged::elements::DenseView;
use crate::staged::typed_elements::{NativeKind, TypedView, ViewKind};
use crate::staged::{CallFrame, Concrete, Constant, ElementFrame, Frame, GlobalFrame, NumericFrame, PropertyFrame, entry};

/// The hole holding the site table an element access reads its shape facts from (a dense access's
/// stage; the entry rows' views are proved by the loop's entry, so they read no site table).
pub(crate) const ELEMENT_SITES: u8 = 3;

/// A row's places with the loop entry's view of the array. Every register-frame trait delegates to
/// `places`; `kept_view` answers the entry's view, and the element hooks `Places` overrides
/// (`array_records`, `index_key`, `array_store_gate`) delegate to it.
pub(crate) struct PlacesView<'a> {
    pub(crate) places: &'a mut Places,
    /// The array register of the row; its frame slot is not read (see the module docs).
    array: Register,
    /// The entry's view of the array, proved by the loop's entering edge.
    view: ElementView<u64>,
}

impl<'a> PlacesView<'a> {
    /// The row's places with the loop entry's `view` of the array in register `array`: the frame a row's own
    /// `ElementRead` or `ElementWrite` runs over (`step`), where the array is not read from its frame slot.
    #[inline(always)]
    pub(crate) fn new(places: &'a mut Places, array: Register, view: ElementView<u64>) -> Self {
        Self { places, array, view }
    }
}

impl<'a> Frame<Concrete> for PlacesView<'a> {
    type Fact = Representation;
    type Control = ();

    #[inline(always)]
    fn fact(&self, register: Register) -> Representation { self.places.fact(register) }

    #[inline(always)]
    fn register_type(&self, register: Register) -> Option<Type> { self.places.register_type(register) }

    #[inline(always)]
    fn read(&mut self, stage: &mut Concrete, register: Register) -> u64 { self.places.read(stage, register) }

    /// The array register answers 0: the entry row's array is not in its frame (see the module docs).
    #[inline(always)]
    fn tagged(&mut self, stage: &mut Concrete, register: Register) -> u64 {
        if register == self.array { 0 } else { self.places.tagged(stage, register) }
    }

    #[inline(always)]
    fn write(&mut self, stage: &mut Concrete, register: Register, value: u64, fact: Representation) {
        self.places.write(stage, register, value, fact)
    }

    #[inline(always)]
    fn this(&mut self, stage: &mut Concrete) -> u64 { self.places.this(stage) }

    #[inline(always)]
    fn define_this(&mut self, stage: &mut Concrete, destination: Register, this: u64) {
        self.places.define_this(stage, destination, this)
    }

    #[inline(always)]
    fn constant(&mut self, stage: &mut Concrete, constant: Constant) -> (u64, Representation) { self.places.constant(stage, constant) }

    #[inline(always)]
    fn truthiness(&mut self, stage: &mut Concrete, register: Register) -> Option<bool> { self.places.truthiness(stage, register) }

    #[inline(always)]
    fn branch(&mut self, stage: &mut Concrete, condition: bool) { self.places.branch(stage, condition) }

    #[inline(always)]
    fn jump(&mut self, stage: &mut Concrete) { self.places.jump(stage) }

    #[inline(always)]
    fn boolean(&mut self, stage: &mut Concrete, condition: bool) -> (u64, Representation) { self.places.boolean(stage, condition) }

    #[inline(always)]
    fn load_local(&mut self, stage: &mut Concrete, slot: u16, miss: Option<()>) -> Option<(u64, Representation)> {
        self.places.load_local(stage, slot, miss)
    }

    #[inline(always)]
    fn store_local(&mut self, stage: &mut Concrete, slot: u16, value: u64, miss: Option<()>) -> bool {
        self.places.store_local(stage, slot, value, miss)
    }

    #[inline(always)]
    fn heap_records(&mut self, stage: &mut Concrete) -> Option<(u64, u64, u64)> { self.places.heap_records(stage) }
}

impl<'a> NumericFrame<Concrete> for PlacesView<'a> {
    #[inline(always)]
    fn int32_operand(&mut self, stage: &mut Concrete, operand: Operand, miss: ()) -> Option<crate::staged::Int32<u64>> {
        self.places.int32_operand(stage, operand, miss)
    }

    #[inline(always)]
    fn int_operand(&mut self, stage: &mut Concrete, operand: Operand, miss: ()) -> Option<crate::staged::Int32<u64>> {
        self.places.int_operand(stage, operand, miss)
    }

    #[inline(always)]
    fn number_operand(&mut self, stage: &mut Concrete, operand: Operand, miss: ()) -> Option<crate::staged::Float64<u64>> {
        self.places.number_operand(stage, operand, miss)
    }

    #[inline(always)]
    fn prove_number(&mut self, stage: &mut Concrete, register: Register, miss: ()) -> Option<()> {
        self.places.prove_number(stage, register, miss)
    }

    #[inline(always)]
    fn miss(&self) {}

    #[inline(always)]
    fn int_fact(&self) -> Representation { self.places.int_fact() }

    #[inline(always)]
    fn number_fact(&self) -> Representation { self.places.number_fact() }
}

impl<'a> GlobalFrame<Concrete> for PlacesView<'a> {}
impl<'a> CallFrame<Concrete> for PlacesView<'a> {}
impl<'a> PropertyFrame<Concrete> for PlacesView<'a> {
    type Plans = crate::staged::NoPlans;
    fn plans(&self) -> crate::staged::NoPlans { crate::staged::NoPlans }
}

impl<'a> ElementFrame<Concrete> for PlacesView<'a> {
    #[inline(always)]
    fn array_records(&mut self, stage: &mut Concrete) -> Option<u64> {
        ElementFrame::<Concrete>::array_records(&mut *self.places, stage)
    }

    #[inline(always)]
    fn index_key(&mut self, stage: &mut Concrete, key: Operand, miss: ()) -> Option<u64> {
        ElementFrame::<Concrete>::index_key(&mut *self.places, stage, key, miss)
    }

    #[inline(always)]
    fn array_store_gate(&mut self, stage: &mut Concrete, base: u64, value: u64, miss: ()) -> Option<()> {
        ElementFrame::<Concrete>::array_store_gate(&mut *self.places, stage, base, value, miss)
    }

    /// The loop entry's view answers every access of the row, a read and a store alike.
    #[inline(always)]
    fn kept_view(&mut self, _stage: &mut Concrete, _base: u64, _writable: bool) -> Option<ElementView<u64>> {
        Some(self.view)
    }
}

/// The dense view of the array whose entry words lie at the offset the site of hole `H` holds: the
/// backing `(values, length)` the loop's entry proved (`entry::entry_backing`).
#[inline(always)]
pub(crate) fn entry_dense<const H: u8>(payload: usize) -> ElementView<u64> {
    let mut stage = StencilStage::from_hole(H);
    let record = word_at(payload, JS_STEP_LOOP_ENTRY);
    let offset = stage_site(&mut stage);
    let (values, length) = entry::entry_backing(&mut stage, record, offset);
    ElementView::Dense(DenseView { values, length, packed: false, within: false, double: None, uncounted: false })
}

/// The typed view the loop's entry recorded for the array whose entry words lie at the offset of the site
/// at hole `at`, of the kind row `KIND` (counted from `first`) plans: the entry proved the kind once.
#[inline(always)]
pub(crate) fn entry_typed<const KIND: u8>(payload: usize, (at, first): (u8, u8)) -> Option<ElementView<u64>> {
    let native = NativeKind::new(TypedArrayKind::ALL[usize::from(KIND - first)])?;
    let mut stage = StencilStage::from_hole(at);
    let offset = stage_site(&mut stage);
    let record = word_at(payload, JS_STEP_LOOP_ENTRY);
    let values = entry::entry_word(&mut stage, record, offset);
    let length = entry::entry_word(&mut stage, record, offset + LOOP_ENTRY_ARRAY_COUNT_AT);
    Some(ElementView::Typed(TypedView { values, length, kind: ViewKind::Static(native), within: false }))
}

