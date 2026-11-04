//! Canonical Number result laws and register forwarding below lane dispatch.
use super::*;
use super::super::operations::{ADDITION, DIVISION, MULTIPLICATION, SUBTRACTION, number_result_is_commutative, number_value_operator};

fn operator(destination: u16, left: u16, right: u16) -> LaneInstruction {
    LaneInstruction {
        handler: std::ptr::null(), operand: LaneOperand { imm: 0 },
        a: destination, b: left, c: right,
        call_link: std::cell::Cell::new(NO_CALL_LINK),
    }
}

#[test]
fn canonical_number_commutativity_covers_signed_zero_nan_and_boundaries() {
    const MIN_SUBNORMAL: f64 = f64::MIN_POSITIVE * f64::EPSILON;
    assert!(number_result_is_commutative(ADDITION));
    assert!(number_result_is_commutative(MULTIPLICATION));
    assert!(!number_result_is_commutative(SUBTRACTION));
    assert!(!number_result_is_commutative(DIVISION));
    let values = [0.0, -0.0, 1.0, -1.0, 0.25, -0.25,
        f64::from(i32::MIN), f64::from(i32::MAX), f64::from(i32::MAX) + 1.0,
        f64::MIN_POSITIVE, MIN_SUBNORMAL, f64::MAX, -f64::MAX,
        f64::INFINITY, f64::NEG_INFINITY, f64::NAN].map(Value::number);
    for left in values {
        for right in values {
            // Value equality checks encoded bits, including the sign of zero and canonical NaN.
            assert_eq!(number_value_operator::<ADDITION>(left, right), number_value_operator::<ADDITION>(right, left));
            assert_eq!(number_value_operator::<MULTIPLICATION>(left, right), number_value_operator::<MULTIPLICATION>(right, left));
        }
    }
}

fn alias_states<const OP: u32>() {
    const REGISTERS: u16 = 3;
    let initial = [Value::integer(2), Value::number(3.5), Value::UNDEFINED];
    for update in 0..REGISTERS {
        for read in 0..REGISTERS {
            for left in 0..REGISTERS {
                for right in 0..REGISTERS {
                    if left != read && right != read { continue; }
                    let other = if left == read { right } else { left };
                    for destination in 0..REGISTERS {
                        let op = operator(destination, left, right);
                        for element in [Value::integer(i32::MAX), Value::number(-0.0), Value::number(0.25),
                            Value::number(f64::NAN), Value::UNDEFINED, Value::NULL, Value::boolean(true)] {
                            let mut expected = initial;
                            let mut actual = initial;
                            let baseline = Registers(expected.as_mut_ptr());
                            let candidate = Registers(actual.as_mut_ptr());
                            for registers in [baseline, candidate] {
                                registers.set(update, Value::integer(1));
                                registers.set(read, element);
                            }
                            let operands = read_number_operands::<OP>(candidate, &op, other, element);
                            let before = number_value_operator::<OP>(baseline.get(left), baseline.get(right));
                            let after = number_value_operator::<OP>(operands.0, operands.1);
                            assert_eq!(before, after);
                            if !number_result_is_commutative(OP) {
                                assert_eq!(operands, (baseline.get(left), baseline.get(right)));
                            }
                            if let Some(value) = before { baseline.set(destination, value); }
                            if let Some(value) = after { candidate.set(destination, value); }
                            assert_eq!(expected, actual);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn read_number_forwarding_preserves_aliases_ordered_operations_and_rejection_state() {
    alias_states::<ADDITION>();
    alias_states::<MULTIPLICATION>();
    alias_states::<SUBTRACTION>();
    alias_states::<DIVISION>();
}

#[cfg(feature = "bench-primitives")]
mod measurements {
    extern crate test;
    use super::*;
    use std::hint::black_box;
    use test::Bencher;

    const OPERATIONS: usize = 64;
    const READ: u16 = 0;
    const OTHER: u16 = 1;
    const RESULT: u16 = 2;

    #[inline(always)]
    fn step<const OP: u32, const FORWARD: bool>(r: Registers, op: &LaneInstruction, other: u16, element: Value) -> Value {
        r.set(READ, element);
        let (left, right) = if FORWARD { read_number_operands::<OP>(r, op, other, element) }
            else { (r.get(op.b), r.get(op.c)) };
        let result = number_value_operator::<OP>(left, right).unwrap();
        r.set(op.a, result);
        result
    }

    fn batch<const OP: u32, const FORWARD: bool, const CHAIN: bool, const BOTH: bool>(b: &mut Bencher) {
        let other = if BOTH { READ } else { OTHER };
        // The read is on the right, so forwarding also exercises canonical operand order.
        let op = operator(RESULT, other, READ);
        let initial = Value::number(0.25);
        let mut storage = [Value::UNDEFINED, Value::number(0.5), Value::UNDEFINED];
        let r = Registers(storage.as_mut_ptr());
        let inputs: [Value; OPERATIONS] = std::array::from_fn(|index| Value::number(index as f64 + 0.25));
        b.iter(|| {
            let r = black_box(r);
            let op = black_box(&op);
            let other = black_box(other);
            let mut value = black_box(initial);
            for &input in &inputs {
                value = step::<OP, FORWARD>(r, op, other, black_box(if CHAIN { value } else { input }));
                black_box(value);
            }
            black_box(&storage);
        });
    }

    macro_rules! kernels {
        ($($name:ident: $op:ident, $forward:literal, $chain:literal, $both:literal;)*) => {$(
            #[bench]
            fn $name(b: &mut Bencher) { batch::<$op, $forward, $chain, $both>(b); }
        )*};
    }

    kernels! {
        primitive_commute_add_reload_batch: ADDITION, false, false, false;
        primitive_commute_add_forward_batch: ADDITION, true, false, false;
        primitive_commute_add_reload_chain_batch: ADDITION, false, true, false;
        primitive_commute_add_forward_chain_batch: ADDITION, true, true, false;
        primitive_commute_add_reload_both_batch: ADDITION, false, false, true;
        primitive_commute_add_forward_both_batch: ADDITION, true, false, true;
        primitive_commute_multiply_reload_batch: MULTIPLICATION, false, false, false;
        primitive_commute_multiply_forward_batch: MULTIPLICATION, true, false, false;
        primitive_commute_multiply_reload_chain_batch: MULTIPLICATION, false, true, false;
        primitive_commute_multiply_forward_chain_batch: MULTIPLICATION, true, true, false;
        primitive_commute_multiply_reload_both_batch: MULTIPLICATION, false, false, true;
        primitive_commute_multiply_forward_both_batch: MULTIPLICATION, true, false, true;
    }
}
