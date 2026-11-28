//! The property rows' frame: a row's places wrapped with the plan its holes name and the VM
//! callout its site answers, so a property row runs its staged `Operation` (`GetField`,
//! `SetField`, `StoreField`, `FieldGetHost`) through `step_property`, the same definition the
//! interpreter and the specializers run (`staged/property.rs`, `staged/property_write.rs`).
//!
//! `PlannedPlaces` holds `&mut Places` and its row's `[IcOp; N]` plan. Every register-frame trait
//! delegates to `places`. The property traits answer the row's plan (`RowPlans`, a copy of the
//! wrapper's own `ops`, so the `FieldPlan` it hands out borrows that copy for the read) and the
//! callouts below. Each callout reads the row's holes through `stage.site(..)` in the order the
//! hand row read them, so the hole order of every row is unchanged.

use crate::bytecode::{FieldBase, FieldLookup, Operand, Register};
use crate::codes::CHAIN_SECOND_AT;
use crate::heap_rows::{context_word, payload, stage_site, word_at};
use crate::layout::{CONTEXT_THIS, JS_STEP_FIELD_ENTRIES, JS_STEP_LOOP_ENTRY};
use crate::typed_ir::Representation;
use crate::places::{DEST, LEFT, Places};
use crate::staged::ic::IcOp;
use crate::staged::property::{FieldPlan, PlanSource, PrototypeProof, StorePlan};
use crate::staged::region::Region;
use crate::staged::{CallFrame, Concrete, Constant, ElementFrame, Frame, GlobalBinding, GlobalFrame, NumericFrame, Operation, PropertyFrame, Stage, Step, entry, lexical_read, step_names_of};
use crate::words::{HOLE_B, S, hole64};

/// Bits in a method-cache bitset word, as a shift.

/// The VM callout a row's property frame answers beside its plan.
#[derive(Clone, Copy)]
pub(crate) enum Callout {
    /// No callout: the row's plan is the whole read.
    Plan,
    /// The loop entry's field at the entry record's byte offset (`FIELD_ROW`).
    LoopEntry,
    /// The site's cache entry read (`SITE_ROW`, `THIS_SITE_ROW`); `chained` reads a nested pair of
    /// entries (`ENTRY`).
    SiteRead { chained: bool },
    /// The site's cache entry store (`SITE_STORE_ROW`, `THIS_STORE_ROW`), through the store gate
    /// where `gated`.
    SiteStore { gated: bool },
    /// The own flat-slot store of a cache row (`IC_SET_ROW`, `IC_SET_INT_ROW`), of an int32 only
    /// where `int`.
    FlatStore { int: bool },
}

/// A prototype-held read a row folds (`HELD_ROW`), its operands read from the row's holes: the receiver
/// word, the exact shape and prototype link hole 1 packs (low and high half), and the held value (hole 2).
#[derive(Clone, Copy)]
pub(crate) struct HeldRead {
    pub(crate) object: u64,
    pub(crate) shape: u32,
    pub(crate) link: u32,
    /// The held value, paired with `link` as the plan's constant.
    pub(crate) constant: [(u32, u64); 1],
}

/// The plan a row's holes name, copied out of the frame for one staged read or store.
#[derive(Clone, Copy)]
pub(crate) struct RowPlans<const N: usize> {
    ops: [IcOp<'static>; N],
    held: Option<HeldRead>,
}

impl<const N: usize> PlanSource for RowPlans<N> {
    #[inline(always)]
    fn field_plan(&self, _lookup: FieldLookup) -> Option<FieldPlan<'_>> {
        Some(match &self.held {
            Some(held) => FieldPlan::Inherited { shape: held.shape, constants: &held.constant[..],
                proof: PrototypeProof::Linked { expected_link: held.link } },
            None => FieldPlan::Ops(&self.ops[..]),
        })
    }

    /// Every store of a row is the VM's callout (`StorePlan::Host`): the row's own store is
    /// `Callout::FlatStore`, `SiteStore`, which the frame answers through `field_store_host`.
    #[inline(always)]
    fn store_plan(&self, _site: u16) -> Option<StorePlan<'_>> {
        Some(StorePlan::Host)
    }
}

/// A row's places with its plan and callout. The other families' frame hooks (elements, calls) are
/// not reached by the property operations a row runs, so their impls take the defaults. The global
/// hooks answer only where `lexical` names a global lexical binding's address (the lexical rows):
/// the name read is then `GlobalBinding::Lexical` and the store writes that binding.
pub(crate) struct PlannedPlaces<'a, const N: usize> {
    pub(crate) places: &'a mut Places,
    pub(crate) ops: [IcOp<'static>; N],
    pub(crate) callout: Callout,
    pub(crate) lexical: Option<u64>,
    /// The prototype-held read the row folds (`HELD_ROW`), whose receiver is `field_this`.
    pub(crate) held: Option<HeldRead>,
    /// The receiver word register `LEFT` reads, where the row's holes name it elsewhere than hole 1: an IC
    /// get's receiver sits above the destination in hole 0 (`site_object`), and hole 1 is the upper-word table.
    pub(crate) receiver: Option<u64>,
}

impl<'a, const N: usize> Frame<Concrete> for PlannedPlaces<'a, N> {
    type Fact = Representation;
    type Control = ();

    #[inline(always)]
    fn fact(&self, register: Register) -> Representation { self.places.fact(register) }

    #[inline(always)]
    fn read(&mut self, stage: &mut Concrete, register: Register) -> u64 {
        match self.receiver {
            Some(receiver) if register == LEFT => receiver,
            _ => self.places.read(stage, register),
        }
    }

    #[inline(always)]
    fn tagged(&mut self, stage: &mut Concrete, register: Register) -> u64 {
        match self.receiver {
            Some(receiver) if register == LEFT => receiver,
            _ => self.places.tagged(stage, register),
        }
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

impl<'a, const N: usize> NumericFrame<Concrete> for PlannedPlaces<'a, N> {
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

impl<'a, const N: usize> GlobalFrame<Concrete> for PlannedPlaces<'a, N> {
    /// The lexical binding the row names (`lexical`), else the dynamic global the row's read answers.
    #[inline(always)]
    fn global_binding(&mut self, _proof: ()) -> GlobalBinding {
        match self.lexical {
            Some(address) => GlobalBinding::Lexical { address },
            None => GlobalBinding::Dynamic,
        }
    }

    /// The word of the lexical binding at `address`, read live and defined into `destination`; `None`
    /// where the binding is in its dead zone (the VM raises).
    #[inline(always)]
    fn global_lexical(&mut self, stage: &mut Concrete, destination: Register, address: u64, _proof: ()) -> Option<()> {
        let word = lexical_read(stage, address, ())?;
        Frame::write(self, stage, destination, word, Representation::Tagged);
        Some(())
    }

    /// The store of `value` to the lexical binding the row names: the binding must be initialized
    /// (`lexical_read`), and its cell is one write (the record is a root the collector scans whole).
    #[inline(always)]
    fn global_write(&mut self, stage: &mut Concrete, _source: Register, _atom: u32, _site: u16, value: u64, _initialize: bool) -> Option<()> {
        let address = self.lexical?;
        lexical_read(stage, address, ())?;
        stage.store_u64(Region::Cells.mutable(), address, 0, value);
        Some(())
    }
}
impl<'a, const N: usize> ElementFrame<Concrete> for PlannedPlaces<'a, N> {}
impl<'a, const N: usize> CallFrame<Concrete> for PlannedPlaces<'a, N> {}

impl<'a, const N: usize> PropertyFrame<Concrete> for PlannedPlaces<'a, N> {
    type Plans = RowPlans<N>;

    #[inline(always)]
    fn plans(&self) -> RowPlans<N> {
        RowPlans { ops: self.ops, held: self.held }
    }

    /// The activation's bound receiver, from the payload (the `THIS_*` rows); a held read's receiver.
    #[inline(always)]
    fn field_this(&mut self, _stage: &mut Concrete, _miss: ()) -> Option<u64> where Self: Sized {
        match self.held {
            Some(held) => Some(held.object),
            None => Some(context_word(self.places.s.frame, CONTEXT_THIS)),
        }
    }

    /// The loop entry's field (`FIELD_ROW`): the entry record's word at hole 1.
    #[inline(always)]
    fn entry_field(&mut self, stage: &mut Concrete, _lookup: FieldLookup) -> Option<u64> {
        match self.callout {
            Callout::LoopEntry => {
                let record = word_at(payload(&self.places.s), JS_STEP_LOOP_ENTRY);
                let offset = stage_site(stage);
                Some(entry::entry_word(stage, record, offset))
            }
            _ => None,
        }
    }

    /// The VM's cached read of the site (`FieldGetHost`), through the site's cache entry.
    #[inline(always)]
    fn field_get_host(&mut self, stage: &mut Concrete, object: u64, _atom: u32, _site: u16, miss: ()) -> Option<u64> {
        let Callout::SiteRead { chained } = self.callout else { return None };
        let entries = field_tables(&self.places.s)?;
        // No function record table (`entry::readable_record`): a read of a function's own field misses, so
        // the call that follows runs the interpreter's builtin row (265 Ir) and not a body's native call
        // (`region_call_in_lane`, 1,250 Ir); it takes the table once a body's native call costs no more.
        let tables = (entries, 0);
        if chained {
            let offsets = hole64::<HOLE_B>();
            entry::site_chain(stage, tables, object, (offsets & u64::from(u32::MAX), offsets >> CHAIN_SECOND_AT), miss)
        } else {
            let offset = stage_site(stage);
            entry::site_read(stage, tables, object, offset, miss)
        }
    }

    /// The VM's own-field store of `value` (the store plan's host join): the site's cache entry
    /// store, or the flat own-slot store of a cache row.
    #[inline(always)]
    fn field_store_host(&mut self, stage: &mut Concrete, object: u64, _atom: u32, _site: u16, value: u64, miss: ()) -> Option<()> {
        match self.callout {
            Callout::SiteStore { gated } => {
                let entries = field_tables(&self.places.s)?;
                let offset = stage_site(stage);
                entry::site_store(stage, entries, object, offset, (value, gated, None), miss)
            }
            Callout::FlatStore { .. } => None,
            Callout::Plan | Callout::LoopEntry | Callout::SiteRead { .. } => None,
        }
    }
}

/// The planned frame for a row: `places` wrapped with the plan `ops` and the callout, the frame the rows that
/// run the staged step themselves build (`site_rows`, `ic_rows`), so the row names the `Operation` it runs.
#[inline(always)]
pub(crate) fn planned_frame<'a, const N: usize>(places: &'a mut Places,
    ops: [IcOp<'static>; N], callout: Callout) -> PlannedPlaces<'a, N> {
    PlannedPlaces { places, ops, callout, lexical: None, held: None, receiver: None }
}

/// The staged global-name operation `operation` (`LoadName` or `GlobalWrite`) run over `places` where
/// the name is the global lexical binding at `address` (`GlobalBinding::Lexical`): the step it made.
#[inline(always)]
pub(crate) fn run_lexical<const KIND: u16>(stage: &mut Concrete,
    places: &mut Places, address: u64, operation: Operation<u64, ()>) -> Step<u64, ()> {
    let mut frame = PlannedPlaces { places, ops: [], callout: Callout::Plan, lexical: Some(address), held: None, receiver: None };
    step_names_of::<KIND, _, _>(stage, &mut frame, operation)
}

/// `GetField` of the receiver in register `LEFT` (hole 1), the plan deciding how it reads.
#[inline(always)]
pub(crate) fn receiver_read() -> Operation<u64, ()> {
    Operation::GetField { base: FieldBase::register(LEFT), lookup: FieldLookup::Site(0), destination: DEST }
}

/// `SetField` of the value in register `LEFT` into the object in register `DEST` (hole 0).
#[inline(always)]
pub(crate) fn receiver_set() -> Operation<u64, ()> {
    Operation::SetField { value: LEFT, base: Some(DEST), atom: 0, site: 0, strict: false }
}

/// The field cache entry table of the activation. The entry reads address class cells directly (the object record
/// table is gone), so the table alone is what a site read or store needs.
#[inline(always)]
fn field_tables(s: &S) -> Option<u64> {
    Some(word_at(payload(s), JS_STEP_FIELD_ENTRIES))
}
