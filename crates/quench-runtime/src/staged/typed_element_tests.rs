//! The typed element rows on the concrete stage, per kind, against Rust's own conversions of the
//! element's bytes: a frame that kept a typed view answers `ElementRead` and `ElementWrite`; and
//! the view proof over the native record a published typed array has, per kind.

use super::element_rows::ElementView;
use super::typed_elements::{typed_read, typed_read_integer, NativeKind, TypedView, ViewKind};
use super::typed_view::{concrete_typed_read, concrete_typed_view, prove_typed_view, TypedRecords, ViewAccess};
use super::test_words::{READ, WRITE};
use super::*;
use crate::bytecode::Operand;
use crate::heap::TypedArrayKind as Kind;
use crate::heap::{BufferBytes, BufferViewObject, Cell, Heap, Object, ViewLength};
use crate::typed_ir::{IntRange, Type};
use crate::Value;

/// A frame holding a typed view over `bytes`: register 0 is the array, 1 the int32 key, 2 the
/// Number (its `f64` bits) a store writes.
struct Typed {
    registers: [u64; 3],
    view: TypedView<u64>,
}

impl Frame<Concrete> for Typed {
    type Fact = ();
    type Control = Edge;
    fn fact(&self, _register: Register) {}
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 { self.registers[usize::from(register)] }
    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: ()) {
        self.registers[usize::from(register)] = value;
    }
    fn this(&mut self, _stage: &mut Concrete) -> u64 { Value::DELETED.to_bits() }
    fn define_this(&mut self, _stage: &mut Concrete, _destination: Register, _this: u64) {}
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, ()) { (constant.value.to_bits(), ()) }
    fn truthiness(&mut self, _stage: &mut Concrete, _register: Register) -> Option<bool> { None }
    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> Edge {
        if condition { Edge::Target } else { Edge::Following }
    }
    fn jump(&mut self, _stage: &mut Concrete) -> Edge { Edge::Target }
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) { (Value::boolean(condition).to_bits(), ()) }
}

impl GlobalFrame<Concrete> for Typed {}

impl PropertyFrame<Concrete> for Typed {
    type Plans = NoPlans;
    fn plans(&self) -> NoPlans { NoPlans }
    fn value_fact(&self, _register: Register) -> Type { Type::NUMBER }
}

impl ElementFrame<Concrete> for Typed {
    fn kept_view(&mut self, _stage: &mut Concrete, _base: u64, _writable: bool) -> Option<ElementView<u64>> {
        Some(ElementView::Typed(self.view))
    }
}

impl crate::staged::CallFrame<Concrete> for Typed {}

impl NumericFrame<Concrete> for Typed {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<crate::staged::Int32<u64>> { unreachable!("no int32 operand") }
    fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<crate::staged::Float64<u64>> { Some(crate::staged::Float64::by_fact(self.registers[2], crate::staged::NumberFact::float64_home())) }
    fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { None }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

/// The kinds the rows address, each with the Number its element holds (`read`) and the bytes a
/// store of a Number leaves (`written`), by Rust's conversions.
const KINDS: [Kind; 9] = [Kind::Uint8, Kind::Uint8Clamped, Kind::Int8, Kind::Uint16, Kind::Int16, Kind::Int32, Kind::Uint32,
    Kind::Float32, Kind::Float64];

/// The Number the element of `kind` at the start of `bytes` holds.
fn read(kind: Kind, bytes: &[u8]) -> f64 {
    let word = |count: usize| -> [u8; 8] { let mut word = [0; 8]; word[..count].copy_from_slice(&bytes[..count]); word };
    match kind {
        Kind::Uint8 | Kind::Uint8Clamped => f64::from(bytes[0]),
        Kind::Int8 => f64::from(bytes[0] as i8),
        Kind::Uint16 => f64::from(u16::from_ne_bytes([bytes[0], bytes[1]])),
        Kind::Int16 => f64::from(i16::from_ne_bytes([bytes[0], bytes[1]])),
        Kind::Int32 => f64::from(i32::from_ne_bytes(word(4)[..4].try_into().unwrap())),
        Kind::Uint32 => f64::from(u32::from_ne_bytes(word(4)[..4].try_into().unwrap())),
        Kind::Float32 => f64::from(f32::from_ne_bytes(word(4)[..4].try_into().unwrap())),
        _ => f64::from_ne_bytes(word(8)),
    }
}

/// The bytes of one element of `kind` after a store of `number`.
fn written(kind: Kind, number: f64) -> Vec<u8> {
    let integer = crate::units::to_int32(number);
    match kind {
        Kind::Uint8 | Kind::Int8 => vec![integer as u8],
        Kind::Uint8Clamped => vec![if number.is_nan() { 0 } else { number.clamp(0.0, 255.0).round_ties_even() as u8 }],
        Kind::Uint16 | Kind::Int16 => (integer as u16).to_ne_bytes().to_vec(),
        Kind::Int32 | Kind::Uint32 => integer.to_ne_bytes().to_vec(),
        Kind::Float32 => (number as f32).to_ne_bytes().to_vec(),
        _ => number.to_ne_bytes().to_vec(),
    }
}

/// The row `operation` over `kind`'s elements, `count` of them at `bytes`.
fn run(kind: Kind, bytes: &mut [u8], (key, number): (i32, f64), operation: Operation<u64, ()>) -> Option<u64> {
    let view = TypedView { values: bytes.as_mut_ptr() as u64, length: (bytes.len() / kind.width()) as u64, kind: ViewKind::Dynamic(kind as u64), within: false };
    let mut frame = Typed { registers: [Value::UNDEFINED.to_bits(), Value::integer(key).to_bits(), number.to_bits()], view };
    match step(&mut Concrete::new(std::ptr::null()), &mut frame, operation) {
        Step::Value(word) => Some(word),
        Step::Unresolved => None,
        Step::Control(_) => unreachable!("an element access transfers no control"),
    }
}

/// Elements of `kind` filled with a pattern whose bytes are all distinct and high.
fn pattern(kind: Kind) -> Vec<u8> { (0..kind.width() * 3).map(|byte| 0xF0_u8.wrapping_add(byte as u8 * 7)).collect() }

#[test]
fn a_read_converts_each_kinds_element_to_its_number() {
    for kind in KINDS {
        let mut bytes = pattern(kind);
        for index in 0..3 {
            let expected = read(kind, &bytes[index * kind.width()..]);
            let word = run(kind, &mut bytes, (index as i32, 0.0), READ).expect("an element reads");
            assert_eq!(word, expected.to_bits(), "{kind:?}[{index}]");
        }
    }
}

#[test]
fn a_read_leaves_past_the_elements_at_a_negative_index_and_for_a_kind_without_a_native_element() {
    for kind in KINDS {
        let mut bytes = pattern(kind);
        for key in [3, i32::MAX, -1, i32::MIN] {
            assert_eq!(run(kind, &mut bytes, (key, 0.0), READ), None, "{kind:?}[{key}]");
        }
    }
    let mut bytes = [0_u8; 16];
    let view = TypedView { values: bytes.as_mut_ptr() as u64, length: 1, kind: ViewKind::Dynamic(Kind::BigInt64 as u64), within: false };
    let mut frame = Typed { registers: [0, Value::integer(0).to_bits(), 0], view };
    assert!(matches!(step(&mut Concrete::new(std::ptr::null()), &mut frame, READ), Step::Unresolved));
}

#[test]
fn a_store_converts_the_number_to_each_kinds_element_and_leaves_its_neighbours() {
    let numbers = [0.0, -0.0, 1.5, -1.5, 127.5, 255.5, 256.0, 300.7, 65537.0, 4294967297.0, -2147483649.0, 0.1, 1e300,
        f64::from(f32::MAX) * 2.0, f64::INFINITY, f64::NAN];
    for kind in KINDS {
        for number in numbers {
            let mut bytes = pattern(kind);
            let (before, width) = (bytes.clone(), kind.width());
            assert_eq!(run(kind, &mut bytes, (1, number), WRITE), Some(number.to_bits()), "{kind:?} stores {number}");
            let mut expected = before.clone();
            expected[width..2 * width].copy_from_slice(&written(kind, number));
            assert_eq!(bytes, expected, "{kind:?} stores {number}");
        }
    }
}

/// A typed array of `kind` over a new buffer holding `bytes`, and that buffer, in `heap`.
fn typed_array(heap: &mut Heap, kind: Kind, bytes: Vec<u8>) -> (Value, Value) {
    let length = bytes.len() / kind.width();
    let buffer = heap.alloc(Cell::ArrayBuffer { object: Box::new(Object::new(Value::NULL)), bytes: BufferBytes::from(bytes),
        shared: false, detached: false, max_byte_length: 0, resizable: false, immutable: false });
    let record = Box::new(BufferViewObject { native: crate::heap::NativeView::typed(kind), object: Object::new(Value::NULL), offset: 0 });
    (heap.alloc(Cell::TypedArray { object: record, buffer, length: ViewLength::fixed(length) }), buffer)
}

/// Publishes the typed array `array` over `buffer`'s bytes, `unshared` or not, as the VM does.
fn publish(heap: &mut Heap, (array, buffer): (Value, Value), unshared: bool) {
    let Some(Cell::ArrayBuffer { bytes, .. }) = heap.get_mut(buffer) else { unreachable!("a buffer") };
    let store = bytes.mut_in_lane().expect("a buffer no other handle shares");
    let (data, length) = (store.as_mut_ptr(), store.len());
    let Some(Cell::TypedArray { object, .. }) = heap.get_mut(array) else { unreachable!("a typed array") };
    let width = object.native.kind().width();
    object.native.publish(data, (length / width) as u32, unshared);
}

/// The proof of `base` for `access` over `heap`'s records.
fn proof(heap: &Heap, base: Value, access: ViewAccess) -> Option<TypedView<u64>> {
    let (directory, pages) = heap.typed_view_directory();
    let records = TypedRecords { directory: directory.addr() as u64, pages: pages as u64,
        relocations: crate::heap::buffer_relocations_address().addr() as u64 };
    prove_typed_view(&mut Concrete::new(std::ptr::null()), records, base.to_bits(), access, ())
}

/// How often a proof is tried again: another test's buffer event, which the process's count
/// shares, makes a view stale between its publication and its proof.
const TRIES: usize = 64;

/// The proof after a publication, as often as such an event intervenes.
fn proved(heap: &mut Heap, array: (Value, Value), access: ViewAccess) -> Option<TypedView<u64>> {
    (0..TRIES).find_map(|_| { publish(heap, array, true); proof(heap, array.0, access) })
}

#[test]
fn a_published_views_proof_reads_each_kinds_elements_through_a_site_kind_or_the_records() {
    for kind in KINDS {
        let (mut heap, bytes) = (Heap::new(), pattern(kind));
        let array = typed_array(&mut heap, kind, bytes.clone());
        let native = NativeKind::new(kind).expect("a native kind");
        for access in [ViewAccess::READ, ViewAccess { kind: Some(native), writable: false }, ViewAccess { kind: Some(native), writable: true }] {
            let view = proved(&mut heap, array, access).expect("a published view of the kind proves");
            assert_eq!(view.length, 3, "{kind:?}");
            assert_eq!(matches!(view.kind, ViewKind::Static(_)), access.kind.is_some(), "{kind:?}: a site kind folds");
            for index in 0..3 {
                let word = typed_read(&mut Concrete::new(std::ptr::null()), view, index, ()).expect("an element reads");
                assert_eq!(word, read(kind, &bytes[index as usize * kind.width()..]).to_bits(), "{kind:?}[{index}]");
            }
        }
    }
}

#[test]
fn the_proof_refuses_another_kind_shared_bytes_to_a_store_a_stale_or_withdrawn_view_and_any_other_value() {
    let mut heap = Heap::new();
    assert!(proof(&heap, Value::integer(1), ViewAccess::READ).is_none(), "no typed array yet: no table");
    let array = typed_array(&mut heap, Kind::Float64, pattern(Kind::Float64));
    let site = |kind| ViewAccess { kind: NativeKind::new(kind), writable: false };
    assert!(proof(&heap, array.0, ViewAccess::READ).is_none(), "an unpublished view");
    assert!(proved(&mut heap, array, site(Kind::Float64)).is_some());
    assert!(proved(&mut heap, array, site(Kind::Int32)).is_none(), "another kind");
    publish(&mut heap, array, false);
    assert!(proof(&heap, array.0, ViewAccess { writable: true, ..site(Kind::Float64) }).is_none(), "a store through shared bytes");
    publish(&mut heap, array, true);
    drop(BufferBytes::default());
    assert!(proof(&heap, array.0, ViewAccess::READ).is_none(), "a buffer event since the publication");
    let Some(Cell::TypedArray { object, .. }) = heap.get_mut(array.0) else { unreachable!("a typed array") };
    object.native.retract();
    assert!(proof(&heap, array.0, ViewAccess::READ).is_none(), "a withdrawn view");
    let plain = heap.alloc_array(crate::heap::ArrayObject::new(Object::new(Value::NULL), Vec::<Value>::new()));
    for other in [Value::integer(1), Value::number(1.5), Value::UNDEFINED, array.1, plain] {
        assert!(proof(&heap, other, ViewAccess::READ).is_none(), "{other:?}");
    }
}

#[test]
fn the_frameless_arms_are_the_proof_and_the_row() {
    let mut heap = Heap::new();
    let array = typed_array(&mut heap, Kind::Int16, pattern(Kind::Int16));
    let entries = heap.typed_view_directory();
    assert!(concrete_typed_view(entries, array.0).is_none(), "an unpublished view");
    let view = (0..TRIES).find_map(|_| { publish(&mut heap, array, true); concrete_typed_view(entries, array.0) }).expect("a published view");
    let bytes = pattern(Kind::Int16);
    assert_eq!(concrete_typed_read(view, 1), Some(read(Kind::Int16, &bytes[2..]).to_bits()));
    assert_eq!(concrete_typed_read(view, 3), None, "past the elements");
}

#[test]
fn an_integer_kinds_element_reads_as_its_int32_word_inside_its_range_and_other_kinds_decline() {
    for kind in KINDS {
        let (mut heap, bytes) = (Heap::new(), pattern(kind));
        let array = typed_array(&mut heap, kind, bytes.clone());
        let native = NativeKind::new(kind).expect("a native kind");
        let view = proved(&mut heap, array, ViewAccess { kind: Some(native), writable: false }).expect("a published view of the kind proves");
        for index in 0..3 {
            let number = read(kind, &bytes[index as usize * kind.width()..]);
            let word = typed_read_integer(&mut Concrete::new(std::ptr::null()), view, index, ());
            assert_eq!(word.is_some(), native.is_int32(), "{kind:?}");
            if let Some(word) = word { assert_eq!(f64::from(word as u32 as i32), number, "{kind:?}[{index}]"); }
            if let Some(range) = native.element_type().int_range() {
                assert!(range.contains(IntRange::singleton(number as i64)), "{kind:?}[{index}] = {number} in {range:?}");
            }
        }
    }
}

#[test]
fn element_types_are_the_kinds_exact_integer_ranges_and_numbers_for_floats() {
    let type_of = |kind| NativeKind::new(kind).expect("a native kind").element_type();
    let range = |min, max| Type::int(IntRange { min, max });
    assert_eq!(type_of(Kind::Uint8), range(0, 255));
    assert_eq!(type_of(Kind::Uint8Clamped), range(0, 255));
    assert_eq!(type_of(Kind::Int8), range(-128, 127));
    assert_eq!(type_of(Kind::Uint16), range(0, 65535));
    assert_eq!(type_of(Kind::Int16), range(-32768, 32767));
    assert_eq!(type_of(Kind::Int32), Type::INT32);
    assert_eq!(type_of(Kind::Uint32), range(0, i64::from(u32::MAX)));
    assert!(!type_of(Kind::Uint32).is_int32() && type_of(Kind::Uint32).is_subtype(Type::NUMBER));
    assert_eq!(type_of(Kind::Float32), Type::NUMBER);
    assert_eq!(type_of(Kind::Float64), Type::NUMBER);
}
