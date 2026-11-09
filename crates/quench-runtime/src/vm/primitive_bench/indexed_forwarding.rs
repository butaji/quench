//! Two indexed Number additions below dispatch. Register roles are distinct;
//! production admission/alias analysis is deliberately not implemented here.
use super::*;
use super::super::operations::{ADDITION, immediate_number_pair, number_operator, number_value_operator};
use crate::heap::{ArrayObject, ArrayStorageProofs, Cell, Heap, LaneProofScope, Object};

#[derive(Clone, Copy)]
#[repr(u16)]
enum Slot { FirstBase, SecondBase, FirstIndex, SecondIndex, FirstRead, SecondRead, Accumulator, Intermediate, Result }
const REGISTER_COUNT: usize = Slot::Result as usize + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Step { First, Second }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Stop { Update(Step), Read(Step), Operator(Step), Complete }

fn initial(first: Value, second: Value, accumulator: Value) -> [Value; REGISTER_COUNT] {
    let mut state = [Value::UNDEFINED; REGISTER_COUNT];
    state[Slot::FirstBase as usize] = first;
    state[Slot::SecondBase as usize] = second;
    state[Slot::FirstIndex as usize] = Value::integer(-1);
    state[Slot::SecondIndex as usize] = Value::integer(-1);
    state[Slot::Accumulator as usize] = accumulator;
    state
}

#[inline(always)]
fn read(proofs: &ArrayStorageProofs, r: Registers, step: Step) -> Result<Value, Stop> {
    let (base, index, result) = match step {
        Step::First => (Slot::FirstBase, Slot::FirstIndex, Slot::FirstRead),
        Step::Second => (Slot::SecondBase, Slot::SecondIndex, Slot::SecondRead),
    };
    let next = integer_increment::<false>(r.get(index as u16)).ok_or(Stop::Update(step))?;
    r.set(index as u16, Value::integer(next));
    let value = usize::try_from(next).ok()
        .and_then(|index| proofs.element(base as u16, r.get(base as u16), index))
        .ok_or(Stop::Read(step))?;
    r.set(result as u16, value);
    Ok(value)
}

#[inline(always)]
fn pair<const FORWARD: bool, const LIVE: bool>(proofs: &ArrayStorageProofs, r: Registers) -> Stop {
    let element = match read(proofs, r, Step::First) { Ok(value) => value, Err(stop) => return stop };
    let accumulator = r.get(Slot::Accumulator as u16);
    let first = if FORWARD {
        let Some(value) = Value::number_pair(accumulator, element).or_else(|| immediate_number_pair(accumulator, element))
            .and_then(|(a, b)| number_operator::<ADDITION>(a, b)) else { return Stop::Operator(Step::First) };
        value
    } else {
        let Some(value) = number_value_operator::<ADDITION>(accumulator, element) else { return Stop::Operator(Step::First) };
        r.set(Slot::Intermediate as u16, value);
        // Unused in the materialized variant; it reads the actual register below.
        0.0
    };
    let second = (|| {
        let element = read(proofs, r, Step::Second)?;
        let result = if FORWARD {
            immediate_number_pair(Value::number(first), element).and_then(|(a, b)| number_operator::<ADDITION>(a, b)).map(Value::number)
        } else {
            number_value_operator::<ADDITION>(r.get(Slot::Intermediate as u16), element)
        };
        result.ok_or(Stop::Operator(Step::Second))
    })();
    // A later fallback must observe the completed first operation, even when the
    // intermediate would be dead on successful completion of the pair.
    if FORWARD && (LIVE || second.is_err()) {
        r.set(Slot::Intermediate as u16, Value::number(first));
    }
    match second {
        Ok(value) => { r.set(Slot::Result as u16, value); Stop::Complete }
        Err(stop) => stop,
    }
}

fn array(heap: &mut Heap, values: &[Value]) -> Value {
    heap.alloc(Cell::Array { object: ArrayObject::new(Object::new(Value::NULL), values.to_vec()) })
}

fn compare(proofs: &ArrayStorageProofs, initial: [Value; REGISTER_COUNT]) -> Stop {
    let mut expected = initial;
    let stop = pair::<false, true>(proofs, Registers(expected.as_mut_ptr()));
    let mut live = initial;
    assert_eq!(pair::<true, true>(proofs, Registers(live.as_mut_ptr())), stop);
    assert_eq!(live, expected, "live intermediate: {stop:?}");
    let mut dead = initial;
    assert_eq!(pair::<true, false>(proofs, Registers(dead.as_mut_ptr())), stop);
    if stop == Stop::Complete {
        // This is the sole unobserved register under the kernel's dead-result contract.
        expected[Slot::Intermediate as usize] = initial[Slot::Intermediate as usize];
    }
    assert_eq!(dead, expected, "dead intermediate: {stop:?}");
    stop
}

#[test]
fn indexed_number_forwarding_restores_every_fallback_boundary() {
    let mut heap = Heap::new();
    let object = heap.alloc(Cell::Object(Object::new(Value::NULL)));
    let mut values = vec![0.0, -0.0, 0.25, -0.25, f64::from(i32::MIN), f64::from(i32::MAX),
        f64::from(i32::MAX) + 1.0, f64::MIN_POSITIVE, f64::MIN_POSITIVE * f64::EPSILON,
        f64::MAX, -f64::MAX, f64::INFINITY, f64::NEG_INFINITY, f64::NAN].into_iter().map(Value::number).collect::<Vec<_>>();
    values.extend([Value::UNDEFINED, Value::NULL, Value::TRUE, object, Value::DELETED]);
    let first = array(&mut heap, &values);
    let second = array(&mut heap, &values);
    let mut proofs = ArrayStorageProofs::new();
    let _scope = LaneProofScope::enter();
    assert!(heap.prove_array_storage(&mut proofs, Slot::FirstBase as u16, first));
    assert!(heap.prove_array_storage(&mut proofs, Slot::SecondBase as u16, second));
    let mut reached = std::collections::BTreeSet::new();
    for &accumulator in &values {
        for first_index in 0..values.len() {
            for second_index in 0..values.len() {
                let mut state = initial(first, second, accumulator);
                state[Slot::FirstIndex as usize] = Value::integer(first_index as i32 - 1);
                state[Slot::SecondIndex as usize] = Value::integer(second_index as i32 - 1);
                reached.insert(compare(&proofs, state));
            }
        }
    }
    for slot in [Slot::FirstIndex, Slot::SecondIndex] {
        for index in [Value::integer(-2), Value::integer(i32::MAX), Value::number(0.5), Value::integer(values.len() as i32)] {
            let mut state = initial(first, second, Value::number(0.25));
            state[slot as usize] = index;
            reached.insert(compare(&proofs, state));
        }
    }
    for slot in [Slot::FirstBase, Slot::SecondBase] {
        let mut state = initial(first, second, Value::number(0.25));
        state[slot as usize] = object;
        reached.insert(compare(&proofs, state));
    }
    proofs.retire();
    assert_eq!(compare(&proofs, initial(first, second, Value::number(0.25))), Stop::Read(Step::First));
    assert!(heap.prove_array_storage(&mut proofs, Slot::FirstBase as u16, first));
    assert_eq!(compare(&proofs, initial(first, second, Value::number(0.25))), Stop::Read(Step::Second));
    assert_eq!(reached, [Stop::Update(Step::First), Stop::Read(Step::First), Stop::Operator(Step::First),
        Stop::Update(Step::Second), Stop::Read(Step::Second), Stop::Operator(Step::Second), Stop::Complete].into_iter().collect());
}

#[cfg(feature = "bench-primitives")]
mod measurements {
    extern crate test;
    use super::*;
    use std::hint::black_box;
    use test::Bencher;
    const BATCH_PAIRS: usize = 64;

    #[derive(Clone, Copy)]
    enum Case { Success, SecondProofMiss, SecondNonNumber }

    fn measure<const FORWARD: bool, const LIVE: bool, const CHAIN: bool>(b: &mut Bencher, case: Case) {
        let mut heap = Heap::new();
        let inputs: [Value; BATCH_PAIRS] = std::array::from_fn(|i| Value::number(i as f64 * 0.0625 + 0.25));
        let second_inputs: [Value; BATCH_PAIRS] = std::array::from_fn(|i| match case {
            Case::SecondNonNumber => Value::UNDEFINED,
            _ => Value::number(-(i as f64) * 0.03125 - 0.125),
        });
        let first = array(&mut heap, &inputs);
        let second = array(&mut heap, &second_inputs);
        let mut proofs = ArrayStorageProofs::new();
        let _scope = LaneProofScope::enter();
        assert!(heap.prove_array_storage(&mut proofs, Slot::FirstBase as u16, first));
        assert!(heap.prove_array_storage(&mut proofs, Slot::SecondBase as u16, second));
        let second = if matches!(case, Case::SecondProofMiss) { first } else { second };
        let state = initial(first, second, Value::number(0.25));
        compare(&proofs, state);
        b.iter(|| {
            let proofs = black_box(&proofs);
            let mut storage = black_box(state);
            let mut carried = Value::number(0.25);
            for &input in &inputs {
                storage[Slot::Accumulator as usize] = if CHAIN { carried } else { input };
                let r = Registers(black_box(&mut storage).as_mut_ptr());
                black_box(pair::<FORWARD, LIVE>(proofs, r));
                carried = r.get(Slot::Result as u16);
            }
            black_box(storage);
        });
        compare(&proofs, state);
    }

    macro_rules! kernels {
        ($($name:ident: $forward:literal, $live:literal, $chain:literal, $case:ident;)*) => {$(
            #[bench]
            fn $name(b: &mut Bencher) { measure::<$forward, $live, $chain>(b, Case::$case); }
        )*};
    }
    kernels! {
        primitive_indexpair_materialized_independent: false, true, false, Success;
        primitive_indexpair_forward_live_independent: true, true, false, Success;
        primitive_indexpair_forward_dead_independent: true, false, false, Success;
        primitive_indexpair_materialized_chain: false, true, true, Success;
        primitive_indexpair_forward_live_chain: true, true, true, Success;
        primitive_indexpair_forward_dead_chain: true, false, true, Success;
        primitive_indexpair_materialized_proof_miss: false, true, false, SecondProofMiss;
        primitive_indexpair_forward_live_proof_miss: true, true, false, SecondProofMiss;
        primitive_indexpair_forward_dead_proof_miss: true, false, false, SecondProofMiss;
        primitive_indexpair_materialized_non_number: false, true, false, SecondNonNumber;
        primitive_indexpair_forward_live_non_number: true, true, false, SecondNonNumber;
        primitive_indexpair_forward_dead_non_number: true, false, false, SecondNonNumber;
    }
}
