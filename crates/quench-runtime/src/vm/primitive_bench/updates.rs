//! Number updates and their indexed-read compositions. Each iteration has 64 steps;
//! setup/proof establishment is excluded. These are helper costs, not lane dispatch.

use super::*;
use super::super::dispatch::number_increment;
use super::super::dispatch_fast::{integer_increment, proof_index};

fn increment<const DECREMENT: bool>(b: &mut Bencher, inputs: Inputs) {
    let values = inputs.values();
    b.iter(|| {
        for &value in &values {
            black_box(number_increment(black_box(value), DECREMENT).unwrap());
        }
    });
}

fn increment_chain<const FRACTION: bool>(b: &mut Bencher) {
    let initial = Value::number(if FRACTION { 0.5 } else { 0.0 });
    b.iter(|| {
        let mut value = black_box(initial);
        for _ in 0..BATCH_VALUES {
            value = number_increment(black_box(value), false).unwrap();
        }
        black_box(value);
    });
}

macro_rules! increments {
    ($($name:ident: $decrement:literal, $input:ident;)*) => {$(
        #[bench]
        fn $name(b: &mut Bencher) { increment::<$decrement>(b, Inputs::$input); }
    )*};
}

increments! {
    primitive_update_increment_integer_batch: false, Integers;
    primitive_update_decrement_integer_batch: true, Integers;
    primitive_update_increment_fraction_batch: false, Fractions;
    primitive_update_increment_mixed_batch: false, Mixed;
    primitive_update_increment_boundary_batch: false, Boundaries;
    primitive_update_decrement_boundary_batch: true, Boundaries;
}

#[bench]
fn primitive_update_increment_integer_chain_batch(b: &mut Bencher) { increment_chain::<false>(b); }

#[bench]
fn primitive_update_increment_fraction_chain_batch(b: &mut Bencher) { increment_chain::<true>(b); }

fn index_decode(b: &mut Bencher, values: [Value; BATCH_VALUES]) {
    b.iter(|| {
        for &value in &values { black_box(proof_index(black_box(value))); }
    });
}

#[bench]
fn primitive_update_index_valid_batch(b: &mut Bencher) {
    index_decode(b, std::array::from_fn(|index| Value::integer(index as i32)));
}

#[bench]
fn primitive_update_index_boundary_batch(b: &mut Bencher) {
    index_decode(b, Inputs::Boundaries.values());
}

// Each element encodes the next pre-increment cursor. Incrementing that loaded cursor
// advances one slot, including the last-to-first edge. This creates a real address
// dependency without timing a modulo operation that the production helper does not own.
fn update_read<const PROVED: bool, const CHAIN: bool, const ADD: bool>(b: &mut Bencher) {
    let values: [Value; BATCH_VALUES] = std::array::from_fn(|index| {
        Value::integer(((index + 1) % BATCH_VALUES) as i32 - 1)
    });
    let mut heap = Heap::new();
    let array = heap.alloc(Cell::Array {
        object: ArrayObject::new(Object::new(Value::NULL), values),
    });
    let mut proofs = Box::new(ArrayStorageProofs::new());
    let _scope = LaneProofScope::enter();
    assert!(heap.prove_array_storage(&mut proofs, 0, array));
    let cursors: [Value; BATCH_VALUES] = std::array::from_fn(|index| Value::integer(index as i32 - 1));
    let initial = Value::integer(-1);
    let addend = Value::number(0.5);
    let step = |cursor| {
        let index = if PROVED {
            usize::try_from(integer_increment::<false>(cursor).unwrap()).unwrap()
        } else {
            proof_index(number_increment(cursor, false).unwrap()).unwrap()
        };
        let loaded = black_box(&*proofs).element(0, black_box(array), index).unwrap();
        if ADD {
            black_box(number_value_operator::<ADDITION>(loaded, black_box(addend)).unwrap());
        }
        loaded
    };
    for (index, &cursor) in cursors.iter().enumerate() { assert_eq!(step(cursor), values[index]); }
    b.iter(|| {
        let mut cursor = black_box(initial);
        for &independent in &cursors {
            cursor = step(black_box(if CHAIN { cursor } else { independent }));
            black_box(cursor);
        }
        black_box(cursor);
    });
    let mut cursor = initial;
    for _ in 0..BATCH_VALUES { cursor = step(cursor); }
    assert_eq!(cursor, initial);
}

macro_rules! reads {
    ($($name:ident: $proved:literal, $chain:literal, $add:literal;)*) => {$(
        #[bench]
        fn $name(b: &mut Bencher) { update_read::<$proved, $chain, $add>(b); }
    )*};
}

reads! {
    primitive_update_read_generic_batch: false, false, false;
    primitive_update_read_proved_batch: true, false, false;
    primitive_update_read_generic_chain_batch: false, true, false;
    primitive_update_read_proved_chain_batch: true, true, false;
    primitive_update_read_add_generic_chain_batch: false, true, true;
    primitive_update_read_add_proved_chain_batch: true, true, true;
}

#[test]
fn primitive_update_boundaries_preserve_number_and_index_contracts() {
    for input in Inputs::Boundaries.numbers() {
        for decrement in [false, true] {
            let expected = input + if decrement { -1.0 } else { 1.0 };
            let actual = number_increment(Value::number(input), decrement).unwrap().as_number().unwrap();
            if expected.is_nan() { assert!(actual.is_nan()); }
            else { assert_eq!(actual.to_bits(), expected.to_bits()); }
        }
    }
    for value in [Value::UNDEFINED, Value::NULL, Value::boolean(false), Value::boolean(true)] {
        assert!(number_increment(value, false).is_none());
        assert!(number_increment(value, true).is_none());
        assert!(proof_index(value).is_none());
    }
    assert_eq!(integer_increment::<false>(Value::integer(i32::MAX)), None);
    assert_eq!(integer_increment::<true>(Value::integer(i32::MIN)), None);
    assert_eq!(proof_index(Value::integer(-1)), None);
    assert_eq!(proof_index(Value::integer(0)), Some(0));
    assert_eq!(proof_index(Value::number(0.5)), None);
}
