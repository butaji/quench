//! The facts of one function, as its lowering reads them (`FunctionFacts`): each query answers from the program's settled
//! facts (`AotFacts`) at the function's own pcs, registers and keys.

use super::accessors;
use super::lengths::IndexRange;
use super::record::ElementStore;
use super::registers::Source;
use super::AotFacts;
use super::super::signatures::{Argument, CallSite};
use crate::staged::effects::{CallForm, Effects, ElementWrites, Signature};
use crate::typed_ir::{ElementClass, Representation};

/// The facts of one function.
pub struct FunctionFacts<'a> {
    pub(super) facts: &'a AotFacts,
    pub(super) function: u32,
}

impl<'a> FunctionFacts<'a> {
    /// The function the plain call at `pc` calls: proven, or a guess (`AotFacts::guessed_calls`: a method
    /// read's, or one read from a live key). A row taking it guards the callee it reaches.
    pub fn call_target(&self, pc: u32) -> Option<u32> {
        self.facts.call_targets.get(&(self.function, pc)).copied()
    }

    /// The functions the call at `pc` may call: its one target, or the closed set of a method read (`call_candidates`).
    pub fn call_targets(&self, pc: u32) -> Vec<u32> {
        if let Some(site) = self.accessor_arms(pc) { return site.arms.iter().map(|arm| arm.function).collect(); }
        match self.facts.call_candidates.get(&(self.function, pc)) {
            Some(set) => set.to_vec(),
            None => self.call_target(pc).into_iter().collect(),
        }
    }

    /// The accessor the field access at `pc` runs, where the census proves one (`accessors::accessor_arms`).
    pub(crate) fn accessor_arms(&self, pc: u32) -> Option<&'a accessors::AccessorArms> {
        self.facts.accessor_arms.get(&(self.function, pc))
    }

    /// Every field access of this function that runs an accessor, by pc (`accessor_arms`).
    pub(crate) fn accessor_sites(&self) -> impl Iterator<Item = (u32, &'a accessors::AccessorArms)> + 'a {
        let facts: &'a AotFacts = self.facts;
        facts.accessor_arms.range((self.function, 0)..=(self.function, u32::MAX)).map(|(&(_, pc), site)| (pc, site))
    }

    /// The function the plain call at `pc` always calls, proven (not a guess under `guessed_calls`): a row may
    /// drop its callee guard for it.
    pub fn proven_call_target(&self, pc: u32) -> Option<u32> {
        self.call_target(pc).filter(|_| !self.facts.guessed_calls.contains(&(self.function, pc)))
    }

    /// The class (constructed function) of every object the named field read or store at `pc` reads or stores,
    /// where the settled types name one (`AotFacts::field_owners`): a guess, held under the layout's guard.
    pub fn field_owner(&self, pc: u32) -> Option<u32> {
        self.access_classes(pc).single().map(|(class, _)| class)
    }

    /// The kind of typed array the element access (a read or a store) at `pc` reads or stores, where its base holds typed
    /// arrays of one native kind and no other array (`typed_ir::View::One`). The element is that kind's: an access may
    /// read it as the kind's element type with no conversion of the stores (`typed_ir::view_element_type`).
    pub fn element_view(&self, pc: u32) -> Option<crate::heap::TypedArrayKind> {
        self.facts.element_views.get(&(self.function, pc)).copied()
    }

    /// The capture slots this function reads from its parent's environment (depth 0), as the engine's capture seeds name them
    /// (`int_captures`, `typed_captures`): each slot with its settled cell (`AotFacts::captured_slot_cell`).
    pub fn capture_cells(&self) -> Vec<(u16, crate::typed_ir::Cell)> {
        self.facts.captures.get(&self.function).map_or_else(Vec::new, |slots| {
            slots.iter().map(|&(owner, slot)| (slot, self.facts.captured_slot_cell(owner, slot))).collect()
        })
    }

    /// The cell register `register` holds just before the instruction at `pc` in the settled solve
    /// (`RegisterCells::at`): its type with its range, the classes of its objects and what its arrays keep.
    /// None where the solve did not settle or no path reaches `pc`. A tier lowers the instruction from it:
    /// a guard the cell decides is never built.
    pub fn cell_at(&self, pc: u32, register: u16) -> Option<crate::typed_ir::Cell> {
        self.facts.registers.get(&self.function)?.at(pc, register)
    }

    /// The interned atoms the string register `register` holds just before `pc` is one of, sorted (`typed_ir::StringFact`),
    /// where the settled solve names some. Each is an interned string, so `===` between two atom-known operands is a
    /// pointer compare (`staged::typing::both_interned`), and a key atom-known here reads like a named property.
    /// Empty where the solve names none, or the register holds no string.
    pub fn string_atoms(&self, pc: u32, register: u16) -> Vec<u32> {
        self.cell_at(pc, register).and_then(|cell| cell.ty.string_fact()).map_or_else(Vec::new, |fact| fact.atoms().to_vec())
    }

    /// The UTF-16 length range of the string register `register` holds just before `pc` (`typed_ir::StringLength`), where
    /// the settled solve reached a string there; `None` otherwise.
    pub fn string_length(&self, pc: u32, register: u16) -> Option<crate::typed_ir::StringLength> {
        self.cell_at(pc, register).and_then(|cell| cell.ty.string_fact()).map(|fact| fact.length())
    }

    /// Whether `===` between the registers `left` and `right` just before `pc` is a pointer compare: both hold interned
    /// atoms (`staged::typing::both_interned`).
    pub fn interned_compare(&self, pc: u32, left: u16, right: u16) -> bool {
        match (self.cell_at(pc, left), self.cell_at(pc, right)) {
            (Some(left), Some(right)) => crate::staged::typing::both_interned(left.ty, right.ty),
            _ => false,
        }
    }

    /// Whether `===` between the string registers `left` and `right` just before `pc` is decided by their facts
    /// (`staged::typing::string_equality`): `Some(outcome)`, or `None` where the facts leave it open.
    pub fn string_equal(&self, pc: u32, left: u16, right: u16) -> Option<bool> {
        let (left, right) = (self.cell_at(pc, left)?, self.cell_at(pc, right)?);
        crate::staged::typing::string_equality(left.ty, right.ty)
    }

    /// The source of the value register `register` holds just before the instruction at `pc` (`RegisterCells::source_at`),
    /// where the solve settled: a binding it was loaded from or a property it was read off, else `Computed`. Computed also
    /// where no path reaches `pc` or the solve did not settle, the answer that assumes nothing about the value's origin.
    pub fn source_at(&self, pc: u32, register: u16) -> Source {
        self.facts.registers.get(&self.function).and_then(|cells| cells.source_at(pc, register)).unwrap_or(Source::Computed)
    }

    /// The cell register `register` holds just after the instruction at `pc` (`RegisterCells::after`): what the instruction
    /// writes it to, so a result's proven type. None where the solve did not settle or no path reaches `pc`.
    pub fn cell_after(&self, pc: u32, register: u16) -> Option<crate::typed_ir::Cell> {
        self.facts.registers.get(&self.function)?.after(pc, register)
    }

    /// The cells of every register at the entry of the block starting at `pc` (`RegisterCells::entry`).
    pub fn block_entry(&self, pc: u32) -> Option<&'a [crate::typed_ir::Cell]> {
        self.facts.registers.get(&self.function)?.entry(pc)
    }

    /// The cell the function's returns join to (`FactKey::Return`): a call of the function defines it, since a call's
    /// result is the join of the returns of the callees it may reach (`types::Types::call`). Any value where the solve
    /// did not settle, so a caller that reads it assumes no more than that.
    pub fn return_cell(&self) -> crate::typed_ir::Cell {
        self.facts.cell(crate::typed_ir::FactKey::Return(self.function)).unwrap_or(crate::typed_ir::Cell::ANY)
    }

    /// The keys every cell of this function's settled solve was read from (`AotFacts::dependencies`): a unit that
    /// trusts `cell_at`/`block_entry` of the function subscribes exactly the live ones (`AotFacts::live`); in a
    /// closed world none is live, so it subscribes nothing and keeps no exit.
    pub fn dependencies(&self) -> &'a [crate::typed_ir::FactKey] {
        self.facts.dependencies.get(&self.function).map_or(&[], |keys| keys)
    }

    /// Whether the instruction at `pc` keeps its result, false only for a call whose result register no read follows
    /// (`Readers::result_dead`): such a call boxes no result. Not a deletion check: a call may be dropped only when its
    /// `site_effects` are none besides the result. Any other instruction, and a function the solve did not settle, keeps it.
    pub fn result_used(&self, pc: u32) -> bool {
        !self.facts.readers.as_ref().is_some_and(|readers| readers.result_dead(self.function, pc))
    }

    /// The method name and key the callee of the call at `pc` was read from (`AotFacts::call_keys`).
    pub fn call_key(&self, pc: u32) -> Option<(Option<u32>, crate::typed_ir::FactKey)> {
        self.facts.call_keys.get(&(self.function, pc)).copied()
    }

    /// The classes of the objects the named field access at `pc` reads or stores, or of the receiver the call at
    /// `pc` passes (`AotFacts::access_classes`): a closed union a class switch dispatches on, a guess.
    pub fn access_classes(&self, pc: u32) -> crate::typed_ir::Classes {
        self.facts.access_classes.get(&(self.function, pc)).copied().unwrap_or(crate::typed_ir::Classes::Many)
    }

    /// The class of every `this` the function's recorded calls pass (`AotFacts::this_classes`): a guess.
    pub fn this_class(&self) -> Option<u32> {
        self.facts.cell(crate::typed_ir::FactKey::Receiver(self.function)).and_then(|cell| cell.classes.single()).map(|(class, _)| class)
    }

    /// The slot of named field `atom` in instances of `class` (its index in `AotFacts::class_layout`), where the layout is
    /// proven: a fixed offset a lowering reads in place of the field's name.
    pub fn field_slot(&self, class: u32, atom: u32) -> Option<u16> {
        let slot = self.facts.class_layout(class)?.iter().position(|&field| field == atom)?;
        u16::try_from(slot).ok()
    }

    /// The function the construction (`new`) at `pc` always constructs.
    pub fn construct_target(&self, pc: u32) -> Option<u32> {
        self.facts.construct_targets.get(&(self.function, pc)).copied()
    }

    /// Per register, the Number representation it always holds.
    pub fn numbers(&self) -> Option<&'a [Option<Representation>]> {
        self.facts.numbers.get(&self.function).map(|numbers| &**numbers)
    }

    /// Whether the index register `index` is below the length of the array register `array` before the instruction at
    /// `pc`, so an access `array[index]` there needs no bounds test: a loop's test `index < array.length` proves it on the
    /// body's edge, and a write to either register or a change to the array's length retires the proof (`lengths`).
    pub fn below_length(&self, pc: u32, index: u16, array: u16) -> bool {
        self.below_length_offset(pc, index, array, 0)
    }

    /// Whether `index + offset` is below the length of the array register `array` before the instruction at `pc`
    /// (`below_length` at `offset` 0): the access `array[index + offset]` needs no bounds test there. A sum the index
    /// register holds (`index = base + k`) is proven through its base.
    pub fn below_length_offset(&self, pc: u32, index: u16, array: u16, offset: u32) -> bool {
        self.facts.lengths.get(&self.function).is_some_and(|settled| settled.proves_below(pc, index, array, i64::from(offset)))
    }

    /// The register the index register `index` is below before the instruction at `pc`, where a comparison that held proves
    /// `index < bound` (`lengths::refine`). A loop's entry compares the bound's reach with an array's length once (`bound_below`).
    pub fn bound_below(&self, pc: u32, index: u16) -> Option<u16> {
        self.facts.lengths.get(&self.function)?.bound_below(pc, index)
    }

    /// The range of the index the element access at `pc` reads, where its floor and ceiling are forms over registers that no write in
    /// a loop breaks: `header` is that loop's header pc. The lane evaluates `floor` and `ceiling` once on the loop's entering edge and
    /// compares them with 0 and the array's length; `nan` says the index may be NaN, which the lane takes separately. The analysis
    /// never checks the range in execution, and a read with no such loop has none.
    pub fn index_ceiling(&self, pc: u32) -> Option<IndexRange> {
        self.facts.lengths.get(&self.function)?.index_range(pc)
    }

    /// Whether the step at `pc` may resize an array of element class `array` (a call that may write its elements, a mutator, guest
    /// code, or an element store the relations do not prove within its array), for the arrays the function's registers hold.
    /// Where the solve recorded no lengths, the step may resize.
    pub fn may_resize_length(&self, pc: u32, array: ElementClass) -> bool {
        self.facts.lengths.get(&self.function).is_none_or(|settled| settled.may_resize(pc, array))
    }

    /// Whether the step at `pc` may resize an array of any element class (`may_resize_length` over every class).
    pub fn may_resize_any(&self, pc: u32) -> bool {
        self.facts.lengths.get(&self.function).is_none_or(|settled| settled.may_resize_any(pc))
    }

    /// Whether no index accessor can run for an element access of the program (`AotFacts::index_accessors_absent`).
    pub(super) fn index_accessors_absent(&self) -> bool {
        self.facts.index_accessors_absent
    }

    /// Whether no builtin prototype object has an index property (`AotFacts::prototype_indices_empty`).
    pub(crate) fn prototype_indices_empty(&self) -> bool {
        self.facts.prototype_indices_empty()
    }

    /// The call `pc` makes, as its callee sees its arguments (`AotFacts::call_sites`); None where the solve read no such call.
    pub(crate) fn call_site(&self, pc: u32) -> Option<&'a CallSite> {
        self.facts.call_sites.get(&(self.function, pc))
    }

    /// The array element store at `pc` writes and the indices it touches (`AotFacts::element_stores`), where the solve kept it.
    pub(crate) fn element_store(&self, pc: u32) -> Option<ElementStore> {
        self.facts.element_stores.get(&(self.function, pc)).copied()
    }

    /// Whether the arguments `first` and `second` of the call at `pc` are arrays of apart classes (`AotFacts::element_classes_distinct`):
    /// a write through one leaves the other's elements, and the facts about them, valid across the call. False where the
    /// call is none the solve read.
    pub fn arguments_distinct(&self, pc: u32, first: usize, second: usize) -> bool {
        match self.call_site(pc).map(|site| (site.arguments.get(first).copied(), site.arguments.get(second).copied())) {
            Some((Some(Argument { class: a, .. }), Some(Argument { class: b, .. }))) => self.facts.element_classes_distinct(a, b),
            _ => false,
        }
    }

    /// Whether the step at `pc` may write an element of an array of class `array`: its graded writes (`site_effects`)
    /// are any, or name a class that is not apart from `array`. A step the solve did not settle may write any.
    pub fn may_write_elements_of(&self, pc: u32, array: ElementClass) -> bool {
        let writes = self.site_effects(pc).elements;
        writes == ElementWrites::Any || writes.entries().iter().any(|write| !self.facts.element_classes_distinct(write.class, array))
    }

    /// Whether the step at `pc` may write the elements of an array of any class (`may_write_elements_of` for every class).
    pub(crate) fn may_write_any_elements(&self, pc: u32) -> bool {
        self.site_effects(pc).elements != ElementWrites::Nothing
    }

    /// The composed effect signature of the function (`signatures::solve`): the leaves its steps can cause and the
    /// effects they have. The same settled facts that answer this file's queries decide it, and the same edges
    /// retire it. Top where the type solve did not settle.
    pub fn signature(&self) -> Signature {
        self.facts.signatures.as_ref().map_or(Signature::ANY, |signatures| signatures.of(self.function))
    }

    /// The graded effects of the step at `pc` (`Signatures::site_effects`): the operation's kind row met with what the
    /// settled facts prove of that site, so a plain read of a closed receiver is not graded as a call. The same facts
    /// and edges as `signature`. Top where the type solve did not settle.
    pub fn site_effects(&self, pc: u32) -> Effects {
        self.facts.signatures.as_ref().map_or(Effects::ANY, |signatures| signatures.site_effects(self.function, pc))
    }

    /// Whether the step at `pc` may allocate (set the collector's due flag): it allocates, or it may run guest code
    /// (getters, setters, valueOf/toString, Proxy, host callbacks), which may allocate (`site_effects`).
    pub fn may_allocate(&self, pc: u32) -> bool {
        let effects = self.site_effects(pc);
        effects.may_alloc || effects.may_call_js
    }

    /// The call form of the call at `pc` (`staged::effects::call_form`): inlined where `inlinable`, else frameless
    /// where the callees the facts name cannot leave, collect or run guest code, else framed. The same settled facts
    /// decide it, and the same edges retire it. Framed where the type solve did not settle.
    pub fn call_form(&self, pc: u32, inlinable: bool) -> CallForm {
        match self.facts.signatures.as_ref() {
            Some(signatures) => signatures.call_form(self.function, pc, inlinable),
            None => crate::staged::effects::call_form(Signature::ANY, inlinable),
        }
    }
}
