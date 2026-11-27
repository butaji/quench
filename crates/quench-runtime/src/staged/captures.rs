//! Capture access as staged rows: a read of the capture `slot`, `depth` scopes out, and a store
//! to it. The frame owns where captures live (the running function's dense slots, the
//! environment chain the VM walks); the row owns the order of effects, so a store reads its
//! source once and a read defines its destination once. A binding the frame's own access does
//! not take (an uninitialized `let`, a `const`, a shared slot) is the interpreter's exact path.

use super::{NumericFrame, NumericStage, Operation, Step};

/// Run the capture row `operation`: its word, or `Unresolved` where the frame's access declines.
#[inline(always)]
pub(super) fn step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::LoadCapture { destination, depth, slot, root } => if const { super::op_kind::admits(KIND, &[super::op_kind::LOAD_CAPTURE]) } { match frame.capture_slot(stage, depth, slot, root) {
            Some((value, fact)) => {
                frame.write(stage, destination, value, fact);
                Step::Value(value)
            }
            None => Step::Unresolved,
        } } else { unreachable!() },
        Operation::StoreCapture { source, depth, slot } => if const { super::op_kind::admits(KIND, &[super::op_kind::STORE_CAPTURE]) } { {
            let value = frame.tagged(stage, source);
            match frame.store_capture(stage, depth, slot, value) {
                Some(()) => Step::Value(value),
                None => Step::Unresolved,
            }
        } } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::LOAD_CAPTURE, super::op_kind::STORE_CAPTURE]) } { Step::Unresolved } else { unreachable!() },
    }
}

/// `step_of` for any operation.
#[inline(always)]
pub(super) fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{Op, Operand, Register, WideInstruction};
    use crate::staged::{Concrete, Constant, Edge, Frame, GlobalFrame, NoPlans, PropertyFrame};
    use crate::Value;

    /// A register file over a function's capture slots, as the interpreter's access sees them:
    /// an uninitialized slot (`Value::DELETED`, a marker) takes neither a read nor a store.
    struct CaptureFrame {
        registers: Vec<u64>,
        slots: Vec<u64>,
    }

    impl CaptureFrame {
        /// The slot the access takes: in range, and no internal marker.
        fn bound(&self, depth: u16, slot: u16) -> Option<usize> {
            let at = usize::from(slot);
            (depth == 0 && self.slots.get(at).is_some_and(|word| !Value::from_bits(*word).is_internal_marker())).then_some(at)
        }
    }

    impl Frame<Concrete> for CaptureFrame {
        type Fact = ();
        type Control = Edge;
        fn fact(&self, _register: Register) {}
        fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 { self.registers[usize::from(register)] }
        fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: ()) {
            self.registers[usize::from(register)] = value;
        }
        fn this(&mut self, _stage: &mut Concrete) -> u64 { Value::DELETED.to_bits() }
        fn define_this(&mut self, _stage: &mut Concrete, destination: Register, this: u64) {
            self.registers[usize::from(destination)] = this;
        }
        fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, ()) { (constant.value.to_bits(), ()) }
        fn truthiness(&mut self, _stage: &mut Concrete, _register: Register) -> Option<bool> { None }
        fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> Edge {
            if condition { Edge::Target } else { Edge::Following }
        }
        fn jump(&mut self, _stage: &mut Concrete) -> Edge { Edge::Target }
        fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) { (Value::boolean(condition).to_bits(), ()) }
        fn capture_slot(&mut self, _stage: &mut Concrete, depth: u16, slot: u16, root: bool) -> Option<(u64, ())> {
            self.bound(depth, slot).filter(|_| !root).map(|at| (self.slots[at], ()))
        }
        fn store_capture(&mut self, _stage: &mut Concrete, depth: u16, slot: u16, value: u64) -> Option<()> {
            self.bound(depth, slot).map(|at| self.slots[at] = value)
        }
    }

    impl GlobalFrame<Concrete> for CaptureFrame {}

    impl crate::staged::CallFrame<Concrete> for CaptureFrame {}
    impl crate::staged::ElementFrame<Concrete> for CaptureFrame {}
    impl PropertyFrame<Concrete> for CaptureFrame {
        type Plans = NoPlans;
        fn plans(&self) -> NoPlans { NoPlans }
    }

    impl crate::staged::NumericFrame<Concrete> for CaptureFrame {
        fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<u64> { unreachable!("no Number operand") }
        fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<u64> { unreachable!("no Number operand") }
        fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { None }
        fn miss(&self) {}
        fn int_fact(&self) {}
        fn number_fact(&self) {}
    }

    fn frame(slots: &[Value]) -> CaptureFrame {
        CaptureFrame { registers: vec![0; 3], slots: slots.iter().map(|value| value.to_bits()).collect() }
    }

    fn run(frame: &mut CaptureFrame, operation: Operation<u64, ()>) -> Step<u64, Edge> {
        crate::staged::step(&mut Concrete::pure(), frame, operation)
    }

    /// The row reads what the slots hold, through the frame's access, and a slot the access
    /// declines (a marker, past the run, a deeper scope) defines nothing.
    #[test]
    fn a_capture_read_defines_its_register_from_the_slot_or_declines() {
        let slots = [Value::number(2.5), Value::DELETED, Value::UNDEFINED];
        for (depth, slot) in [(0, 0), (0, 1), (0, 2), (0, 3), (1, 0)] {
            let mut capture = frame(&slots);
            let probe = (depth == 0).then(|| slots.get(usize::from(slot)).copied()).flatten().filter(|value| !value.is_internal_marker());
            let step = run(&mut capture, Operation::LoadCapture { destination: 1, depth, slot, root: false });
            match (step, probe) {
                (Step::Value(word), Some(expected)) => {
                    assert_eq!(word, expected.to_bits(), "slot {slot} depth {depth}");
                    assert_eq!(capture.registers[1], word);
                }
                (Step::Unresolved, None) => assert_eq!(capture.registers[1], 0, "a declined read defines nothing"),
                _ => panic!("slot {slot} depth {depth} disagrees with the probe"),
            }
        }
    }

    /// A root read is the frame's to answer: a frame with no projected binding declines it, and
    /// the same slot read as a function's own capture defines its register.
    #[test]
    fn a_root_capture_read_asks_the_frame_for_the_root_binding() {
        let mut capture = frame(&[Value::number(2.5)]);
        let read = |root| Operation::LoadCapture { destination: 1, depth: 0, slot: 0, root };
        assert!(matches!(run(&mut capture, read(true)), Step::Unresolved));
        assert_eq!(capture.registers[1], 0, "a declined root read defines nothing");
        assert!(matches!(run(&mut capture, read(false)), Step::Value(word) if word == Value::number(2.5).to_bits()));
    }

    #[test]
    fn a_capture_store_writes_the_source_once_or_declines() {
        let mut capture = frame(&[Value::UNDEFINED, Value::DELETED]);
        capture.registers[0] = Value::number(7.0).to_bits();
        assert!(matches!(run(&mut capture, Operation::StoreCapture { source: 0, depth: 0, slot: 0 }),
            Step::Value(word) if word == Value::number(7.0).to_bits()));
        assert_eq!(capture.slots[0], Value::number(7.0).to_bits());
        assert!(matches!(run(&mut capture, Operation::StoreCapture { source: 0, depth: 0, slot: 1 }), Step::Unresolved));
        assert_eq!(capture.slots[1], Value::DELETED.to_bits(), "an uninitialized binding keeps its marker");
    }

    /// `LoadCapture` and `StoreCapture` decode to the rows, a returning read to none.
    #[test]
    fn the_capture_opcodes_decode_to_their_rows() {
        let imm = crate::bytecode::ImmediateLayout::capture_immediate(1, 4);
        let load = Operation::<u64, ()>::decode(WideInstruction::new(Op::LoadCapture, 2, 0, 0, imm), crate::staged::DecodeContext::EMPTY);
        assert!(matches!(load, Some(Operation::LoadCapture { destination: 2, depth: 1, slot: 4, root: false })));
        let store = Operation::<u64, ()>::decode(WideInstruction::new(Op::StoreCapture, 3, 0, 0, imm), crate::staged::DecodeContext::EMPTY);
        assert!(matches!(store, Some(Operation::StoreCapture { source: 3, depth: 1, slot: 4 })));
        let returning = WideInstruction::new(Op::LoadCapture, 2 | crate::bytecode::RETURN_REGISTER, 0, 0, imm);
        assert!(Operation::<u64, ()>::decode(returning, crate::staged::DecodeContext::EMPTY).is_none());
    }
}
