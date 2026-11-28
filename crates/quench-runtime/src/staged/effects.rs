//! What an operation reads, writes and may do, as data (Stage v2): the alias regions a native
//! load or store belongs to, the effect row of an `Operation`. A
//! specializer reads them to keep facts across operations (a fact about a region survives
//! until an operation that writes the region), and the interpreter ignores them.

use super::{Allocation, CallOperation, Operation};

pub use super::region::Region;

use crate::typed_ir::ElementClass;

#[path = "effects/affine.rs"]
mod affine;
#[path = "effects/writes.rs"]
mod writes;
#[path = "effects/summary.rs"]
mod summary;
#[path = "effects/call_sig.rs"]
mod call_sig;
#[path = "effects/operation_effects.rs"]
mod operation_effects;
pub use affine::*;
pub use writes::*;
pub use summary::*;
pub use call_sig::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Operand;

    #[test]
    fn only_operations_that_store_allocate_or_run_guest_code_move_arrays() {
        type Row = Operation<u64, ()>;
        let stay: [Row; 5] = [Operation::ElementRead { base: 0, key: Operand(1), destination: 0 }, Operation::Move { destination: 0, source: 1 },
            Operation::Jump, Operation::Safepoint { proof: () }, Operation::LoadThis { destination: 0 }];
        assert!(stay.iter().all(|row| !row.moves_arrays()));
        let move_arrays: [Row; 4] = [Operation::ElementWrite { value: 0, base: 1, key: 2, strict: false }, Operation::IndexWrite { index: 0, value: 0, proof: () },
            Operation::Allocate { destination: 0, allocation: Allocation::Empty, proof: () }, Operation::Binary { destination: 0, left: crate::bytecode::Operand::register(1), right: crate::bytecode::Operand::register(2), operator: 0 }];
        assert!(move_arrays.iter().all(Row::moves_arrays));
    }

    #[test]
    fn element_writes_join_by_class_and_widen_past_the_bound() {
        let whole = |class| ClassWrite { class, indices: Indices::Whole };
        let span = |class| ClassWrite { class, indices: Indices::Span { start: Affine::parameter(1), len: Affine::constant(1) } };
        let written = ElementWrites::of([whole(ElementClass::Global(3)), span(ElementClass::Parameter { function: 2, index: 0 })]);
        assert_eq!(written.entries().len(), 2);
        assert_eq!(written.join(ElementWrites::Nothing), written);
        assert_eq!(written.join(ElementWrites::Any), ElementWrites::Any);
        // The same class twice joins its indices: equal spans stay, different ones widen to `Whole`.
        let joined = ElementWrites::of([span(ElementClass::Unnamed), span(ElementClass::Unnamed)]);
        assert_eq!(joined.entries()[0].indices, Indices::Span { start: Affine::parameter(1), len: Affine::constant(1) });
        let widened = ElementWrites::of([span(ElementClass::Unnamed), whole(ElementClass::Unnamed)]);
        assert_eq!(widened.entries()[0].indices, Indices::Whole);
        let many = (0..=MAX_WRITE_CLASSES as u32).map(|function| whole(ElementClass::Return(function)));
        assert_eq!(ElementWrites::of(many), ElementWrites::Any);
        assert_eq!(ElementWrites::of(std::iter::empty()), ElementWrites::Nothing);
    }

    #[test]
    fn a_grade_narrows_element_writes_and_keeps_its_flags() {
        let store = ElementWrites::of([ClassWrite { class: ElementClass::Field(4), indices: Indices::Whole }]);
        let graded = Effects::ANY.meet(Effects { elements: store, ..Effects::ANY });
        assert_eq!(graded.elements, store);
        assert!(graded.may_call_js && graded.may_throw && graded.may_collect, "an element store's grade clears no flag");
        assert_eq!(Effects::NONE.meet(Effects::ANY).elements, ElementWrites::Nothing);
    }

    #[test]
    fn a_callee_index_is_read_through_the_arguments_parameters() {
        let index = Indices::Span { start: Affine::parameter(1), len: Affine::constant(1) };
        let kept = index.substitute(&|param: u16| (param == 1).then_some(0_u16));
        assert_eq!(kept, Indices::Span { start: Affine::parameter(0), len: Affine::constant(1) });
        assert_eq!(index.substitute(&|_: u16| None::<u16>), Indices::Whole, "an argument that is no parameter of the caller widens the index");
    }

    #[test]
    fn a_returning_call_with_nothing_after_it_is_a_tail_site() {
        assert!(tail_site(true, true));
        assert!(!tail_site(false, true), "a call whose value the caller keeps working on is not a tail site");
        assert!(!tail_site(true, false), "a handler or a sloppy caller must still see the call's completion");
    }
}
