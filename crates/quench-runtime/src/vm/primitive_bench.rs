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

#[path = "../../benches/support/number_inputs.rs"]
mod number_inputs;
use number_inputs::{BATCH_VALUES, INPUT_SEED, Inputs};
mod fields;
mod field_reads;
mod captures;
mod strings;
mod updates;
#[cfg(feature = "bench-alloc-routes")]
mod growth;
const PAIR_INITIAL: f64 = 17.5;
const PAIR_FACTOR: f64 = 0.5;

fn call_boundary<const NATIVE: bool, const BODY: bool, const SPECIALIZED: bool>(b: &mut Bencher) {
    let compile = if SPECIALIZED {
        crate::Engine::specialize
    } else {
        crate::Engine::specialize_unspecialized
    };
    let program = compile(
        "function identity(x) { return x; } var nativeAbs = Math.abs;",
        "primitive-call.js",
    ).unwrap();
    let mut vm = super::Vm::new(crate::test_host::TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Interpreted);
    vm.execute(&program).unwrap();
    let name = vm.lookup_atom(if NATIVE { "nativeAbs" } else { "identity" }).unwrap();
    let callee = vm.own_property(vm.realm.globals, name).unwrap();
    let inputs: [Value; BATCH_VALUES] = std::array::from_fn(|index| {
        Value::integer(-(index as i32) - 1)
    });
    let call = |vm: &mut super::Vm<crate::test_host::TestHost>, input| {
        if BODY {
            vm.call_native(&program, black_box(crate::heap::Native::MathAbs), Value::UNDEFINED, &[input])
        } else {
            vm.call_value(&program, black_box(callee), Value::UNDEFINED, &[input])
        }.unwrap()
    };
    let validate = |vm: &mut super::Vm<crate::test_host::TestHost>| {
        for (index, input) in inputs.iter().copied().enumerate() {
            let expected = if NATIVE { Value::integer(index as i32 + 1) } else { input };
            assert_eq!(call(vm, input), expected);
        }
        assert!(vm.active_call_roots.is_empty());
        assert!(vm.frames.is_empty());
        assert!(vm.realm.promise.active_native.is_empty());
    };
    validate(&mut vm);
    b.iter(|| {
        for input in inputs {
            black_box(call(black_box(&mut vm), black_box(input)));
        }
    });
    validate(&mut vm);
}

// The native body includes its rooting policy and interrupt check. It omits
// callee lookup, realm/native activation and stack guarding of the full call.
#[bench]
fn primitive_call_native_abs_batch(b: &mut Bencher) { call_boundary::<true, false, false>(b); }
#[bench]
fn primitive_call_native_abs_body_batch(b: &mut Bencher) { call_boundary::<true, true, false>(b); }
#[bench]
fn primitive_call_user_identity_general_batch(b: &mut Bencher) { call_boundary::<false, false, false>(b); }
#[bench]
fn primitive_call_user_identity_specialized_batch(b: &mut Bencher) { call_boundary::<false, false, true>(b); }

fn native_arity<const DERIVED: bool>(b: &mut Bencher) {
    use crate::heap::Native;
    let natives: Vec<_> = (0..Native::COUNT)
        .map(|index| Native::from_discriminant(index).unwrap())
        .collect();
    b.iter(|| {
        for &native in &natives {
            let native = black_box(native);
            black_box(if DERIVED {
                super::promise::native_length(native)
            } else {
                super::promise::native_length_definition(native)
            });
        }
    });
}

// Each iteration visits every Native once, including kinds with no own length.
#[bench]
fn primitive_native_arity_derived_all(b: &mut Bencher) { native_arity::<true>(b); }

#[bench]
fn primitive_native_arity_definition_all(b: &mut Bencher) { native_arity::<false>(b); }

fn capture_read<const REMEMBERED: bool>(b: &mut Bencher, depth: u16) {
    let mut vm = super::Vm::new(crate::test_host::TestHost::default());
    let expected = Value::integer(42);
    let mut env = Value::NULL;
    for _ in 0..=depth {
        env = vm.heap.alloc_environment(
            env, 1, vec![expected].into(),
            crate::heap::EnvironmentScopeRef::of_program(0),
        ).0;
    }
    vm.walked_capture_slots(env, false, depth).unwrap();
    let read = |vm: &super::Vm<crate::test_host::TestHost>, env, depth| {
        if REMEMBERED {
            vm.remembered_capture(env, false, depth, 0)
        } else {
            vm.environment_capture_slots(env, false, depth)
                .and_then(|(slots, root)| {
                    assert!(root.is_none());
                    slots.dense()?.dense_plain(0)
                })
        }
    };
    assert_eq!(read(&vm, env, depth), Some(expected));
    b.iter(|| {
        for _ in 0..BATCH_VALUES {
            black_box(read(black_box(&vm), black_box(env), black_box(depth)));
        }
    });
    assert_eq!(read(&vm, env, depth), Some(expected));
}

#[bench]
fn primitive_capture_walk_depth_one_batch(b: &mut Bencher) { capture_read::<false>(b, 1); }
#[bench]
fn primitive_capture_walk_depth_four_batch(b: &mut Bencher) { capture_read::<false>(b, 4); }
#[bench]
fn primitive_capture_remembered_depth_one_batch(b: &mut Bencher) { capture_read::<true>(b, 1); }
#[bench]
fn primitive_capture_remembered_depth_four_batch(b: &mut Bencher) { capture_read::<true>(b, 4); }

fn property_store(b: &mut Bencher, tracking: bool, initial: Value) {
    let mut arena = crate::value_vec::ValueArena::default();
    if tracking { arena.track(); }
    const SHAPE: u32 = 1;
    arena.register_derived_shape(SHAPE, BATCH_VALUES, None);
    let values = [initial; BATCH_VALUES];
    let vector = arena.with_values(SHAPE, &values);
    arena.note_shape_values(SHAPE, &values);
    b.iter(|| {
        for slot in 0..BATCH_VALUES {
            let value = black_box(Value::integer(slot as i32));
            let arena = black_box(&mut arena);
            arena.note_shape_value(SHAPE, slot, value);
            // SAFETY: setup allocated all BATCH_VALUES slots in this flat vector.
            unsafe { arena.set_flat_unchecked(vector, slot, value) };
        }
    });
    for slot in 0..BATCH_VALUES {
        assert_eq!(arena.get(vector, slot), Some(Value::integer(slot as i32)));
    }
}

#[bench]
fn primitive_property_store_untracked_batch(b: &mut Bencher) {
    property_store(b, false, Value::integer(0));
}

#[bench]
fn primitive_property_store_integer_tracked_batch(b: &mut Bencher) {
    property_store(b, true, Value::integer(0));
}

#[bench]
fn primitive_property_store_tagged_tracked_batch(b: &mut Bencher) {
    property_store(b, true, Value::UNDEFINED);
}

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

impl Inputs {
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
        for (index, value) in values.iter().enumerate() {
            assert!(black_box(&mut *proofs).replace(
                0,
                black_box(array),
                black_box(index),
                black_box(*value)
            ));
        }
    });
}

#[derive(Clone, Copy)]
enum ArrayStoreKinds {
    Integers,
    Fractions,
    Alternating,
}

/// Two complete passes per iteration: 128 stores, plus 128 reads in the chain cases.
/// Every element encodes its successor, so the dependent cases traverse the same cycle
/// regardless of kind. Alternating passes change every element's kind in both directions;
/// stable passes use the same traversal, payloads and proof checks without kind changes.
fn array_proved_store_pattern<const DEPENDENT: bool, const INTEGER_INDEX: bool>(b: &mut Bencher, kinds: ArrayStoreKinds) {
    assert!(!INTEGER_INDEX || matches!(kinds, ArrayStoreKinds::Integers));
    let successors = std::array::from_fn::<_, BATCH_VALUES, _>(|index| (index + 1) % BATCH_VALUES);
    let integers = successors.map(|index| Value::integer(index as i32));
    let fractions = successors.map(|index| Value::number(index as f64 + 0.5));
    let writes = match kinds {
        ArrayStoreKinds::Integers => [integers, integers],
        ArrayStoreKinds::Fractions => [fractions, fractions],
        ArrayStoreKinds::Alternating => [integers, fractions],
    };
    let mut heap = Heap::new();
    let array = heap.alloc(Cell::Array {
        object: ArrayObject::new(Object::new(Value::NULL), writes[1]),
    });
    let mut proofs = Box::new(ArrayStorageProofs::new());
    let _scope = LaneProofScope::enter();
    assert!(heap.prove_array_storage(&mut proofs, 0, array));
    let mut cursor = 0;
    b.iter(|| {
        for values in &writes {
            for position in 0..BATCH_VALUES {
                let index = if DEPENDENT { cursor } else { position };
                if DEPENDENT {
                    // Setup encodes a nonnegative successor index with an optional half.
                    // Truncation decodes that link; this is part of the composed cost.
                    let next = black_box(&*proofs)
                        .element(0, black_box(array), black_box(index))
                        .unwrap();
                    cursor = if INTEGER_INDEX {
                        super::dispatch_fast::proof_index(next).unwrap()
                    } else {
                        next.as_number().unwrap() as usize
                    };
                }
                assert!(black_box(&mut *proofs).replace(
                    0, black_box(array), black_box(index), black_box(values[index]),
                ));
            }
        }
    });
    assert_eq!(cursor, 0);
    let record = heap.record_array(array).unwrap();
    assert_eq!(&**record.elements(), &writes[1]);
    assert_eq!(record.elements().holds_only_int32(), matches!(kinds, ArrayStoreKinds::Integers));
}

macro_rules! array_store_patterns {
    ($($name:ident: $dependent:literal, $kinds:ident;)*) => {$(
        #[bench]
        fn $name(b: &mut Bencher) {
            array_proved_store_pattern::<$dependent, false>(b, ArrayStoreKinds::$kinds);
        }
    )*};
}

array_store_patterns! {
    primitive_array_proved_store_integer_pair_batch: false, Integers;
    primitive_array_proved_store_fraction_pair_batch: false, Fractions;
    primitive_array_proved_store_kind_transition_pair_batch: false, Alternating;
    primitive_array_proved_read_store_integer_chain_pair_batch: true, Integers;
    primitive_array_proved_read_store_fraction_chain_pair_batch: true, Fractions;
    primitive_array_proved_read_store_kind_transition_chain_pair_batch: true, Alternating;
}

#[bench]
fn primitive_array_proved_read_store_exact_index_chain_pair_batch(b: &mut Bencher) {
    array_proved_store_pattern::<true, true>(b, ArrayStoreKinds::Integers);
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
