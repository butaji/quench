//! The typed element rows on the concrete stage, per kind, against Rust's own conversions of the
//! element's bytes: a frame that kept a typed view answers `ElementRead` and `ElementWrite`.

use super::element_rows::ElementView;
use super::typed_elements::TypedView;
use super::*;
use crate::bytecode::Operand;
use crate::heap::TypedArrayKind as Kind;
use crate::typed_ir::Type;
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

impl NumericFrame<Concrete> for Typed {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> u64 { unreachable!("no int32 operand") }
    fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> u64 { self.registers[2] }
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
    let view = TypedView { values: bytes.as_mut_ptr() as u64, length: (bytes.len() / kind.width()) as u64, kind: kind as u64 };
    let mut frame = Typed { registers: [Value::UNDEFINED.to_bits(), Value::integer(key).to_bits(), number.to_bits()], view };
    match step(&mut Concrete::new(std::ptr::null()), &mut frame, operation) {
        Step::Value(word) => Some(word),
        Step::Unresolved => None,
        Step::Control(_) => unreachable!("an element access transfers no control"),
    }
}

const READ: Operation<u64, ()> = Operation::ElementRead { base: 0, key: Operand(1) };
const WRITE: Operation<u64, ()> = Operation::ElementWrite { value: 2, base: 0, key: 1 };

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
    let view = TypedView { values: bytes.as_mut_ptr() as u64, length: 1, kind: Kind::BigInt64 as u64 };
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
