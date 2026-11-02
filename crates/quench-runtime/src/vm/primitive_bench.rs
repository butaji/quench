//! Direct shared-primitive measurements, below the guest microbench composition layer.
//! Scalar kernels process `BATCH_VALUES` operations per libtest iteration; allocation and
//! COW kernels process one array. Pair kernels process `BATCH_VALUES` two-operation pairs.
//! Setup is outside `Bencher::iter`. Linear TOD-9 owns the
//! suite mapping, commands, interpretation and evidence.

extern crate test;

use super::DataAttributes;
use super::operations::{ADDITION, MULTIPLICATION, integer_operator, number_value_operator};
use crate::heap::{
    ArrayElements, ArrayObject, ArrayStorageProofs, Cell, Heap, LaneProofScope, Object,
};
use crate::value::Value;
use std::hash::{Hash, Hasher};
use std::hint::black_box;
use test::Bencher;

const BATCH_VALUES: usize = 64;
const INPUT_SEED: i32 = 104729;
const FRACTION_DIVISOR: f64 = 31.0;
const PAIR_INITIAL: f64 = 17.5;
const PAIR_FACTOR: f64 = 0.5;

fn attribute_hash<const FIELDS: bool, const DEPENDENT: bool>(b: &mut Bencher) {
    const FLAG_STATES: [bool; 2] = [false, true];
    let states = FLAG_STATES.len();
    let flags: [DataAttributes; BATCH_VALUES] = std::array::from_fn(|index| DataAttributes {
        writable: FLAG_STATES[index % states],
        enumerable: FLAG_STATES[index / states % states],
        configurable: FLAG_STATES[index / (states * states) % states],
    });
    let mut initial = rustc_hash::FxHasher::default();
    black_box(INPUT_SEED).hash(&mut initial);
    b.iter(|| {
        let mut chain = initial.clone();
        for attributes in flags {
            let attributes = black_box(attributes);
            let mut independent = initial.clone();
            let state = if DEPENDENT {
                &mut chain
            } else {
                &mut independent
            };
            if FIELDS {
                (
                    attributes.writable,
                    attributes.enumerable,
                    attributes.configurable,
                )
                    .hash(state);
            } else {
                attributes.hash(state);
            }
            if !DEPENDENT {
                black_box(state.finish());
            }
        }
        if DEPENDENT {
            black_box(chain.finish());
        }
    });
}

#[bench]
fn primitive_attribute_hash_batch(b: &mut Bencher) {
    attribute_hash::<false, false>(b);
}

#[bench]
fn primitive_attribute_hash_fields_batch(b: &mut Bencher) {
    attribute_hash::<true, false>(b);
}

#[bench]
fn primitive_attribute_hash_chain_batch(b: &mut Bencher) {
    attribute_hash::<false, true>(b);
}

#[bench]
fn primitive_attribute_hash_fields_chain_batch(b: &mut Bencher) {
    attribute_hash::<true, true>(b);
}

/// Two shared arithmetic primitives, retaining the intermediate as a Number or a raw
/// double as the lane's dead-intermediate pair does. Each batch is 64 arithmetic pairs.
fn number_pairs<const ELIDE: bool, const CHAIN: bool>(b: &mut Bencher) {
    let values = Inputs::Fractions.values();
    let initial = Value::number(PAIR_INITIAL);
    let factor = Value::number(PAIR_FACTOR);
    b.iter(|| {
        let mut result = black_box(initial);
        for &right in &values {
            let left = black_box(if CHAIN { result } else { initial });
            let right = black_box(right);
            let factor = black_box(factor);
            result = if ELIDE {
                let (left, right) = Value::number_pair(left, right).unwrap();
                let first = super::operations::number_operator::<ADDITION>(left, right).unwrap();
                Value::number(
                    super::operations::number_operator::<MULTIPLICATION>(
                        first,
                        factor.as_number().unwrap(),
                    )
                    .unwrap(),
                )
            } else {
                let first = number_value_operator::<ADDITION>(left, right).unwrap();
                number_value_operator::<MULTIPLICATION>(first, factor).unwrap()
            };
            if !CHAIN {
                black_box(result);
            }
        }
        if CHAIN {
            black_box(result);
        }
    });
}

#[bench]
fn primitive_number_pair_packed_batch(b: &mut Bencher) {
    number_pairs::<false, false>(b);
}

#[bench]
fn primitive_number_pair_elided_batch(b: &mut Bencher) {
    number_pairs::<true, false>(b);
}

#[bench]
fn primitive_number_pair_packed_chain_batch(b: &mut Bencher) {
    number_pairs::<false, true>(b);
}

#[bench]
fn primitive_number_pair_elided_chain_batch(b: &mut Bencher) {
    number_pairs::<true, true>(b);
}

#[bench]
fn primitive_control_number_pair_chain_batch(b: &mut Bencher) {
    let values = Inputs::Fractions.numbers();
    b.iter(|| {
        let mut result = black_box(PAIR_INITIAL);
        for &right in &values {
            result = (result + black_box(right)) * black_box(PAIR_FACTOR);
        }
        black_box(result);
    });
}

#[derive(Clone, Copy)]
enum Inputs {
    Integers,
    WideIntegers,
    LargeInt32,
    Fractions,
    Dyadic,
    Mixed,
    Boundaries,
}

impl Inputs {
    fn numbers(self) -> [f64; BATCH_VALUES] {
        let seed = black_box(INPUT_SEED);
        let edges = [
            0.0,
            -0.0,
            f64::from(i32::MIN),
            f64::from(i32::MAX),
            f64::from(i32::MIN) - 1.0,
            f64::from(i32::MAX) + 1.0,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ];
        std::array::from_fn(|index| {
            let integer = f64::from(seed * (index as i32 - BATCH_VALUES as i32 / 2));
            match self {
                Self::Integers => integer,
                Self::WideIntegers => integer * f64::from(u32::MAX),
                Self::LargeInt32 => f64::from(i32::MAX - seed * index as i32),
                Self::Fractions => integer + 1.0 / FRACTION_DIVISOR,
                Self::Dyadic => integer + 0.5,
                Self::Mixed if index % 2 == 0 => integer,
                Self::Mixed => integer + 1.0 / FRACTION_DIVISOR,
                Self::Boundaries => edges[index % edges.len()],
            }
        })
    }

    fn values(self) -> [Value; BATCH_VALUES] {
        self.numbers().map(Value::number)
    }
}

fn encode(b: &mut Bencher, inputs: Inputs) {
    let numbers = inputs.numbers();
    b.iter(|| {
        for &number in &numbers {
            black_box(Value::number(black_box(number)));
        }
    });
}

fn decode(b: &mut Bencher, inputs: Inputs) {
    let values = inputs.values();
    b.iter(|| {
        for &value in &values {
            black_box(black_box(value).as_number().unwrap());
        }
    });
}

fn length_encode<const DIRECT: bool>(b: &mut Bencher) {
    let values: [usize; BATCH_VALUES] = std::array::from_fn(|index| {
        let value = INPUT_SEED.unsigned_abs() as usize * index;
        if index % 2 == 0 {
            value
        } else {
            value + i32::MAX as usize
        }
    });
    b.iter(|| {
        for &value in &values {
            let value = black_box(value);
            black_box(if DIRECT {
                Value::length(value)
            } else {
                Value::number(value as f64)
            });
        }
    });
}

#[bench]
fn primitive_length_encode_batch(b: &mut Bencher) {
    length_encode::<true>(b);
}

#[bench]
fn primitive_length_encode_via_number_batch(b: &mut Bencher) {
    length_encode::<false>(b);
}

fn arithmetic<const OP: u32>(b: &mut Bencher, inputs: Inputs) {
    let values = inputs.values();
    b.iter(|| {
        for &value in &values {
            black_box(number_value_operator::<OP>(black_box(value), black_box(value)).unwrap());
        }
    });
}

fn integer_arithmetic<const OP: u32>(b: &mut Bencher, inputs: Inputs) {
    let values = inputs.values();
    b.iter(|| {
        for &value in &values {
            black_box(integer_operator::<OP>(black_box(value), black_box(value)).unwrap());
        }
    });
}

fn integer_pair_decode<const COMBINED: bool>(b: &mut Bencher, inputs: Inputs) {
    let values = inputs.values();
    b.iter(|| {
        for index in 0..BATCH_VALUES {
            let left = black_box(values[index]);
            let right = black_box(values[BATCH_VALUES - 1 - index]);
            black_box(if COMBINED {
                Value::int_pair(left, right)
            } else {
                left.as_int()
                    .and_then(|left| right.as_int().map(|right| (left, right)))
            });
        }
    });
}

macro_rules! input_benches {
    ($($name:ident: $operation:ident $(::<$parameter:tt>)? ($input:ident)),* $(,)?) => {
        $(#[bench]
        fn $name(b: &mut Bencher) {
            $operation $(::<$parameter>)? (b, Inputs::$input);
        })*
    };
}

input_benches! {
    primitive_number_encode_int32_batch: encode(Integers),
    primitive_number_encode_wide_integer_batch: encode(WideIntegers),
    primitive_number_encode_fraction_batch: encode(Fractions),
    primitive_number_encode_dyadic_batch: encode(Dyadic),
    primitive_number_encode_mixed_batch: encode(Mixed),
    primitive_number_encode_edges_batch: encode(Boundaries),
    primitive_number_decode_int32_batch: decode(Integers),
    primitive_number_decode_fraction_batch: decode(Fractions),
    primitive_number_decode_mixed_batch: decode(Mixed),
    primitive_number_add_int32_batch: arithmetic::<ADDITION>(Integers),
    primitive_number_add_fraction_batch: arithmetic::<ADDITION>(Fractions),
    primitive_number_add_mixed_batch: arithmetic::<ADDITION>(Mixed),
    primitive_number_multiply_fraction_batch: arithmetic::<MULTIPLICATION>(Fractions),
    primitive_integer_add_batch: integer_arithmetic::<ADDITION>(Integers),
    primitive_integer_add_overflow_batch: integer_arithmetic::<ADDITION>(LargeInt32),
    primitive_integer_multiply_overflow_batch: integer_arithmetic::<MULTIPLICATION>(LargeInt32),
    primitive_integer_pair_combined_batch: integer_pair_decode::<true>(Integers),
    primitive_integer_pair_individual_batch: integer_pair_decode::<false>(Integers),
    primitive_integer_pair_combined_float_batch: integer_pair_decode::<true>(Fractions),
    primitive_integer_pair_individual_float_batch: integer_pair_decode::<false>(Fractions),
    primitive_integer_pair_combined_mixed_batch: integer_pair_decode::<true>(Mixed),
    primitive_integer_pair_individual_mixed_batch: integer_pair_decode::<false>(Mixed),
}

#[bench]
fn primitive_control_f64_bits_batch(b: &mut Bencher) {
    let numbers = Inputs::Fractions.numbers();
    b.iter(|| {
        for &number in &numbers {
            black_box(black_box(number).to_bits());
        }
    });
}

#[bench]
fn primitive_control_f64_add_batch(b: &mut Bencher) {
    let numbers = Inputs::Fractions.numbers();
    b.iter(|| {
        for &number in &numbers {
            black_box(black_box(number) + black_box(number));
        }
    });
}

#[bench]
fn primitive_slot_narrow_encode_batch(b: &mut Bencher) {
    let values = Inputs::Integers.values();
    assert!(values.iter().all(|value| value.to_narrow().is_some()));
    b.iter(|| {
        for &value in &values {
            black_box(black_box(value).to_narrow().unwrap());
        }
    });
}

#[bench]
fn primitive_slot_narrow_decode_batch(b: &mut Bencher) {
    let values = Inputs::Integers
        .values()
        .map(|value| value.to_narrow().unwrap());
    b.iter(|| {
        for &value in &values {
            black_box(Value::from_narrow(black_box(value)));
        }
    });
}

#[bench]
fn primitive_array_backing_read_batch(b: &mut Bencher) {
    let elements = ArrayElements::from(Inputs::Fractions.values());
    b.iter(|| {
        for index in 0..BATCH_VALUES {
            black_box(black_box(&elements)[black_box(index)]);
        }
    });
}

fn record_array(heap: &mut Heap) -> Value {
    heap.alloc(Cell::Array {
        object: ArrayObject::new(Object::new(Value::NULL), Inputs::Fractions.values()),
    })
}

#[bench]
fn primitive_array_record_read_batch(b: &mut Bencher) {
    let mut heap = Heap::new();
    let array = record_array(&mut heap);
    b.iter(|| {
        for index in 0..BATCH_VALUES {
            let record = black_box(&heap).record_array(black_box(array)).unwrap();
            black_box(record.elements()[black_box(index)]);
        }
    });
}

#[bench]
fn primitive_array_proved_read_batch(b: &mut Bencher) {
    let mut heap = Heap::new();
    let array = record_array(&mut heap);
    let mut proofs = Box::new(ArrayStorageProofs::new());
    let _scope = LaneProofScope::enter();
    assert!(heap.prove_array_storage(&mut proofs, 0, array));
    b.iter(|| {
        for index in 0..BATCH_VALUES {
            black_box(
                black_box(&*proofs)
                    .element(0, black_box(array), black_box(index))
                    .unwrap(),
            );
        }
    });
}

#[bench]
fn primitive_array_unique_store_batch(b: &mut Bencher) {
    let values = Inputs::Mixed.values();
    let mut elements = ArrayElements::from(values);
    let unique = elements.make_mut();
    b.iter(|| {
        for index in 0..BATCH_VALUES {
            black_box(&mut *unique).set(
                black_box(index),
                black_box(values[BATCH_VALUES - 1 - index]),
            );
        }
    });
}

#[bench]
fn primitive_array_proved_store_batch(b: &mut Bencher) {
    let mut heap = Heap::new();
    let array = record_array(&mut heap);
    let values = Inputs::Mixed.values();
    let mut proofs = Box::new(ArrayStorageProofs::new());
    let _scope = LaneProofScope::enter();
    assert!(heap.prove_array_storage(&mut proofs, 0, array));
    b.iter(|| {
        for index in 0..BATCH_VALUES {
            assert!(black_box(&mut *proofs).replace(
                0,
                black_box(array),
                black_box(index),
                black_box(values[index])
            ));
        }
    });
}

#[bench]
fn primitive_array_cow_detach_one_array(b: &mut Bencher) {
    let elements = ArrayElements::from(Inputs::Integers.values());
    b.iter(|| {
        let mut copy = black_box(&elements).clone();
        copy.make_mut().set(0, black_box(Value::NULL));
        black_box(copy);
    });
    assert_eq!(elements.owner_count(), 1);
}

#[bench]
fn primitive_array_allocate_one_array(b: &mut Bencher) {
    let values = Inputs::Integers.values();
    b.iter(|| black_box(ArrayElements::from_slice(black_box(&values))));
}
