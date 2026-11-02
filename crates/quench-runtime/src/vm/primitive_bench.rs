//! Direct shared-primitive measurements, below the guest microbench composition layer.
//! Scalar kernels process `BATCH_VALUES` operations per libtest iteration; allocation and
//! COW kernels process one array. Setup is outside `Bencher::iter`. Linear TOD-9 owns the
//! suite mapping, commands, interpretation and evidence.

extern crate test;

use super::operations::{ADDITION, MULTIPLICATION, integer_operator, number_value_operator};
use crate::heap::{ArrayElements, ArrayObject, ArrayStorageProofs, Cell, Heap, LaneProofScope, Object};
use crate::value::Value;
use std::hint::black_box;
use test::Bencher;

const BATCH_VALUES: usize = 64;
const INPUT_SEED: i32 = 104729;
const FRACTION_DIVISOR: f64 = 31.0;

#[derive(Clone, Copy)]
enum Inputs {
    Integers,
    Fractions,
    Dyadic,
    Mixed,
    Boundaries,
}

impl Inputs {
    fn numbers(self) -> [f64; BATCH_VALUES] {
        let seed = black_box(INPUT_SEED);
        let edges = [
            0.0, -0.0, f64::from(i32::MIN), f64::from(i32::MAX),
            f64::from(i32::MIN) - 1.0, f64::from(i32::MAX) + 1.0,
            f64::MIN_POSITIVE, f64::from_bits(1), f64::INFINITY, f64::NEG_INFINITY,
            f64::NAN,
        ];
        std::array::from_fn(|index| {
            let integer = f64::from(seed * (index as i32 - BATCH_VALUES as i32 / 2));
            match self {
                Self::Integers => integer,
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

fn arithmetic<const OP: u32>(b: &mut Bencher, inputs: Inputs) {
    let values = inputs.values();
    b.iter(|| {
        for &value in &values {
            black_box(number_value_operator::<OP>(black_box(value), black_box(value)).unwrap());
        }
    });
}

macro_rules! input_benches {
    ($($name:ident: $operation:ident $(::<$operator:ident>)? ($input:ident)),* $(,)?) => {
        $(#[bench]
        fn $name(b: &mut Bencher) {
            $operation $(::<$operator>)? (b, Inputs::$input);
        })*
    };
}

input_benches! {
    primitive_number_encode_int32_batch: encode(Integers),
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
fn primitive_integer_add_batch(b: &mut Bencher) {
    let values = Inputs::Integers.values();
    b.iter(|| {
        for &value in &values {
            black_box(integer_operator::<ADDITION>(black_box(value), black_box(value)).unwrap());
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
    let values = Inputs::Integers.values().map(|value| value.to_narrow().unwrap());
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
            black_box(black_box(&*proofs).element(0, black_box(array), black_box(index)).unwrap());
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
            black_box(&mut *unique).set(black_box(index), black_box(values[BATCH_VALUES - 1 - index]));
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
            assert!(black_box(&mut *proofs).replace(0, black_box(array), black_box(index), black_box(values[index])));
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
