use super::*;
use crate::heap::{Cell, Heap, Object};
use crate::value_vec::INLINE_PROPERTY_COUNT;
use crate::Value;

/// The storage forms an exact-shape object can take, by how the interpreter's object API
/// (independent of `staged`) describes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Observed {
    Packed,
    Arena,
    DisplacedInline,
    DisplacedArena,
}

fn observed(object: &Object) -> Observed {
    match (object.inline_properties().is_some(), object.plain_property_storage().is_some()) {
        (true, true) => Observed::Packed,
        (false, true) => Observed::Arena,
        (true, false) => Observed::DisplacedInline,
        (false, false) => Observed::DisplacedArena,
    }
}

/// Objects of every storage form, each with its values: narrow integers, narrow references,
/// a double (no narrow form), and enough values to spill past the inline ones.
fn objects_of_every_form(heap: &mut Heap) -> Vec<(Value, Vec<Value>)> {
    let wide = INLINE_PROPERTY_COUNT + 2;
    for shape in 1..=wide {
        heap.register_property_shape(shape as u32, shape, None);
    }
    let reference = heap.alloc(Cell::Error("reference".into()));
    let value_sets: Vec<Vec<Value>> = vec![
        vec![Value::number(7.0), Value::number(-3.0)],
        vec![reference, Value::TRUE],
        vec![Value::number(0.5), Value::NULL],
        (0..wide).map(|index| Value::number(index as f64)).collect(),
    ];
    let mut objects = Vec::new();
    for prototype in [false, true] {
        for values in &value_sets {
            let object = heap.alloc(Cell::Object(Object::new(Value::NULL)));
            for (slot, value) in values.iter().enumerate() {
                heap.property_add(object, *value, slot as u32 + 1);
            }
            if prototype {
                heap.object_record_mut(object).unwrap().mark_as_prototype();
            }
            objects.push((object, values.clone()));
        }
    }
    objects
}

#[test]
fn the_interpreter_reading_of_own_layout_matches_the_object_model_in_every_form() {
    let mut heap = Heap::new();
    let objects = objects_of_every_form(&mut heap);
    let mut forms = std::collections::HashSet::new();
    for (object, values) in objects {
        let data = heap.object(object).unwrap();
        forms.insert(observed(data));
        for (slot, expected) in values.iter().enumerate() {
            // SAFETY: the object's exact shape holds every slot it was given.
            let reference = unsafe { heap.property_get_unchecked(data, slot) };
            assert_eq!(reference, *expected, "{:?} slot {slot}", observed(data));
            // SAFETY: as above; the heap's arena holds the object's run.
            let staged = unsafe { concrete_own_read(data, heap.property_values_base(), slot) };
            assert_eq!(staged, *expected, "{:?} slot {slot}", observed(data));
        }
    }
    for form in [Observed::Packed, Observed::Arena, Observed::DisplacedInline, Observed::DisplacedArena] {
        assert!(forms.contains(&form), "the corpus covers {form:?}: {forms:?}");
    }
}

/// A concrete register frame: the interpreter keeps no facts about its registers. A heap
/// value whose cell decides its truthiness reads the cell through `heap`, as the lane does.
struct ConcreteFrame<'a> {
    registers: Vec<u64>,
    heap: Option<&'a Heap>,
}

impl Frame<Concrete> for ConcreteFrame<'_> {
    type Fact = ();
    type Control = Edge;

    fn fact(&self, _register: Register) -> Self::Fact {}

    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        self.registers[usize::from(register)]
    }

    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: Self::Fact) {
        self.registers[usize::from(register)] = value;
    }

    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, Self::Fact) {
        (constant.value.to_bits(), ())
    }

    fn truthiness(&mut self, stage: &mut Concrete, register: Register) -> Option<bool> {
        let value = Value::from_bits(self.read(stage, register));
        value.truthiness().or_else(|| self.heap?.string(value).map(|text| !text.units().is_empty()))
    }

    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> Edge {
        if condition { Edge::Target } else { Edge::Following }
    }

    fn jump(&mut self, _stage: &mut Concrete) -> Edge {
        Edge::Target
    }
}

impl NumericFrame<Concrete> for ConcreteFrame<'_> {
    fn int_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> u64 {
        let register = operand.register_index().expect("a numeric operand is a register");
        u64::from(Value::from_bits(self.read(stage, register)).as_int().expect("a proved int") as u32)
    }
    fn number_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> u64 {
        let register = operand.register_index().expect("a numeric operand is a register");
        Value::from_bits(self.read(stage, register)).as_number().expect("a proved Number").to_bits()
    }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

/// A Number word a concrete frame holds in one of the two representations the staging
/// names.
#[derive(Clone, Copy, Debug, PartialEq)]
enum NumberWord {
    Int(i32),
    Float(f64),
}

fn int_word(value: i32) -> u64 { u64::from(value as u32) }

/// A concrete frame whose registers hold Number words and whose write fact names their
/// representation, so staged arithmetic defines an int32 or a binary64 result.
struct NumberFrame {
    values: Vec<NumberWord>,
}

impl Frame<Concrete> for NumberFrame {
    type Fact = Domain;
    type Control = Edge;

    fn fact(&self, _register: Register) -> Domain { Domain::Int32 }
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        match self.values[usize::from(register)] {
            NumberWord::Int(value) => int_word(value),
            NumberWord::Float(value) => value.to_bits(),
        }
    }
    fn write(&mut self, _stage: &mut Concrete, register: Register, word: u64, fact: Domain) {
        self.values[usize::from(register)] = match fact {
            Domain::Int32 => NumberWord::Int(word as u32 as i32),
            Domain::Float64 => NumberWord::Float(f64::from_bits(word)),
        };
    }
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, Domain) {
        let value = constant.value;
        match value.as_int() {
            Some(int) => (int_word(int), Domain::Int32),
            None => (value.as_number().expect("a Number constant").to_bits(), Domain::Float64),
        }
    }
    fn truthiness(&mut self, _stage: &mut Concrete, _register: Register) -> Option<bool> {
        unreachable!("a Number operation decides no truthiness")
    }
    fn branch(&mut self, _stage: &mut Concrete, _condition: bool) -> Edge {
        unreachable!("a Number operation branches nowhere")
    }
    fn jump(&mut self, _stage: &mut Concrete) -> Edge {
        unreachable!("a Number operation jumps nowhere")
    }
}

impl NumericFrame<Concrete> for NumberFrame {
    fn int_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> u64 {
        match self.values[usize::from(operand.register_index().expect("a register operand"))] {
            NumberWord::Int(value) => int_word(value),
            NumberWord::Float(value) => int_word(value as i32),
        }
    }
    fn number_operand(&mut self, _stage: &mut Concrete, operand: Operand, _miss: ()) -> u64 {
        match self.values[usize::from(operand.register_index().expect("a register operand"))] {
            NumberWord::Int(value) => f64::from(value).to_bits(),
            NumberWord::Float(value) => value.to_bits(),
        }
    }
    fn miss(&self) {}
    fn int_fact(&self) -> Domain { Domain::Int32 }
    fn number_fact(&self) -> Domain { Domain::Float64 }
}

fn number_frame(left: NumberWord, right: NumberWord) -> NumberFrame {
    NumberFrame { values: vec![left, right, NumberWord::Int(0)] }
}

/// The arithmetic result, or `None` where the operation left at its miss edge.
fn concrete_arithmetic(left: NumberWord, right: NumberWord, operation: NumberArithmetic,
    domain: Domain, overflow: Overflow) -> Option<NumberWord> {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = number_frame(left, right);
    let operation = Operation::NumberArithmetic {
        destination: 2, left: Operand::register(0), right: Operand::register(1),
        operation, domain, overflow,
    };
    match step(&mut stage, &mut frame, operation) {
        Step::Value(_) => Some(frame.values[2]),
        Step::Unresolved => None,
        Step::Control(_) => panic!("arithmetic transfers no control"),
    }
}

fn concrete_relation(left: NumberWord, right: NumberWord, relation: NumberRelation, domain: Domain) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = number_frame(left, right);
    let operation = Operation::NumberRelation {
        left: Operand::register(0), right: Operand::register(1), relation, domain,
    };
    match step(&mut stage, &mut frame, operation) {
        Step::Value(word) => word != 0,
        _ => panic!("a relation produces a predicate"),
    }
}

#[test]
fn staged_int32_arithmetic_matches_the_interpreter_results() {
    use NumberArithmetic::{Add, Multiply, Subtract};
    for (left, right, operation, expected) in [
        (2, 3, Add, 5), (-7, 4, Add, -3), (2, 3, Subtract, -1),
        (-7, -4, Subtract, -3), (6, 7, Multiply, 42), (-6, 7, Multiply, -42),
    ] {
        let result = concrete_arithmetic(NumberWord::Int(left), NumberWord::Int(right),
            operation, Domain::Int32, Overflow::Exit);
        assert_eq!(result, Some(NumberWord::Int(expected)), "{operation:?} {left} {right}");
    }
}

#[test]
fn staged_int32_overflow_leaves_wraps_or_widens() {
    let max = i32::MAX;
    assert_eq!(concrete_arithmetic(NumberWord::Int(max), NumberWord::Int(1),
        NumberArithmetic::Add, Domain::Int32, Overflow::Exit), None);
    assert_eq!(concrete_arithmetic(NumberWord::Int(max), NumberWord::Int(1),
        NumberArithmetic::Add, Domain::Int32, Overflow::Wrap), Some(NumberWord::Int(i32::MIN)));
    assert_eq!(concrete_arithmetic(NumberWord::Int(max), NumberWord::Int(1),
        NumberArithmetic::Add, Domain::Int32, Overflow::Widen),
        Some(NumberWord::Float(f64::from(max) + 1.0)));
    assert_eq!(concrete_arithmetic(NumberWord::Int(0), NumberWord::Int(-1),
        NumberArithmetic::Multiply, Domain::Int32, Overflow::Exit), None);
    let widened = concrete_arithmetic(NumberWord::Int(0), NumberWord::Int(-1),
        NumberArithmetic::Multiply, Domain::Int32, Overflow::Widen);
    assert!(matches!(widened, Some(NumberWord::Float(value)) if value == 0.0 && value.is_sign_negative()));
}

#[test]
fn staged_float_arithmetic_keeps_zero_nan_and_infinity() {
    use NumberArithmetic::{Add, Divide, Multiply, Subtract};
    let float = |operation, left, right| concrete_arithmetic(
        NumberWord::Float(left), NumberWord::Float(right), operation, Domain::Float64, Overflow::Exit);
    assert!(matches!(float(Multiply, -1.0, 0.0), Some(NumberWord::Float(value))
        if value == 0.0 && value.is_sign_negative()));
    assert!(matches!(float(Divide, 0.0, 0.0), Some(NumberWord::Float(value)) if value.is_nan()));
    assert!(matches!(float(Divide, 1.0, 0.0), Some(NumberWord::Float(value)) if value.is_infinite() && value > 0.0));
    assert!(matches!(float(Divide, -1.0, 0.0), Some(NumberWord::Float(value)) if value.is_infinite() && value < 0.0));
    assert_eq!(float(Add, 0.5, 0.25), Some(NumberWord::Float(0.75)));
    assert_eq!(float(Subtract, 0.5, 0.25), Some(NumberWord::Float(0.25)));
}

#[test]
fn staged_mixed_int_float_operands_widen_to_binary64() {
    assert_eq!(concrete_arithmetic(NumberWord::Int(3), NumberWord::Float(0.5),
        NumberArithmetic::Add, Domain::Float64, Overflow::Exit), Some(NumberWord::Float(3.5)));
    assert_eq!(concrete_arithmetic(NumberWord::Float(0.5), NumberWord::Int(4),
        NumberArithmetic::Multiply, Domain::Float64, Overflow::Exit), Some(NumberWord::Float(2.0)));
}

#[test]
fn staged_number_relations_are_ieee_comparisons_in_both_domains() {
    use NumberRelation::{Equal, GreaterEqual, GreaterThan, LessEqual, LessThan, NotEqual};
    for (relation, expected) in [(Equal, false), (NotEqual, true), (LessThan, true),
        (LessEqual, true), (GreaterThan, false), (GreaterEqual, false)] {
        assert_eq!(concrete_relation(NumberWord::Int(2), NumberWord::Int(3), relation, Domain::Int32),
            expected, "int {relation:?}");
        assert_eq!(concrete_relation(NumberWord::Float(2.5), NumberWord::Float(3.5), relation, Domain::Float64),
            expected, "float {relation:?}");
    }
    for relation in [Equal, LessEqual, GreaterEqual] {
        assert!(concrete_relation(NumberWord::Int(2), NumberWord::Int(2), relation, Domain::Int32),
            "int equal {relation:?}");
        assert!(concrete_relation(NumberWord::Float(2.5), NumberWord::Float(2.5), relation, Domain::Float64),
            "float equal {relation:?}");
    }
    let nan = f64::NAN;
    for relation in [Equal, NotEqual, LessThan, LessEqual, GreaterThan, GreaterEqual] {
        let expected = relation == NotEqual;
        assert_eq!(concrete_relation(NumberWord::Float(nan), NumberWord::Float(nan), relation, Domain::Float64),
            expected, "NaN {relation:?}");
    }
    assert!(concrete_relation(NumberWord::Float(-0.0), NumberWord::Float(0.0), Equal, Domain::Float64));
    assert!(concrete_relation(NumberWord::Int(3), NumberWord::Float(3.5), LessThan, Domain::Float64));
    assert!(!concrete_relation(NumberWord::Float(f64::INFINITY), NumberWord::Float(1.0), LessThan, Domain::Float64));
}

#[test]
fn step_moves_and_defines_constants_on_the_concrete_stage() {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![0; 4], heap: None };
    let moved = Value::number(41.0);
    frame.registers[1] = moved.to_bits();
    let Step::Value(word) = step(&mut stage, &mut frame, Operation::Move { destination: 2, source: 1 })
        else { panic!("a move produces a value") };
    assert_eq!(Value::from_bits(word), moved);
    assert_eq!(Value::from_bits(frame.registers[2]), moved);
    let constant = Value::number(7.0);
    let Step::Value(word) = step(&mut stage, &mut frame, Operation::LoadConst {
        destination: 3, constant: Constant::untyped(constant) }) else { panic!("a constant produces a value") };
    assert_eq!(Value::from_bits(word), constant);
    assert_eq!(Value::from_bits(frame.registers[3]), constant);
}

/// The edge a `JumpIf` takes on the concrete stage for `value`.
fn jump_if_edge(value: Value, taken: bool, heap: &Heap) -> Edge {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![value.to_bits()], heap: Some(heap) };
    match step(&mut stage, &mut frame, Operation::JumpIf { register: 0, taken }) {
        Step::Control(edge) => edge,
        Step::Unresolved => panic!("{value:?} needs its cell, which the frame bound"),
        Step::Value(_) => panic!("a jump produces no value"),
    }
}

#[test]
fn step_jumps_on_the_javascript_truthiness_of_every_value_kind() {
    let mut heap = Heap::new();
    let empty = heap.alloc_string("".into());
    let text = heap.alloc_string("x".into());
    let cases = [
        (Value::number(1.0), Edge::Target),
        (Value::number(-3.5), Edge::Target),
        (Value::number(0.0), Edge::Following),
        (Value::number(-0.0), Edge::Following),
        (Value::number(f64::NAN), Edge::Following),
        (Value::TRUE, Edge::Target),
        (Value::FALSE, Edge::Following),
        (Value::NULL, Edge::Following),
        (Value::UNDEFINED, Edge::Following),
        (empty, Edge::Following),
        (text, Edge::Target),
    ];
    for (value, truthy_edge) in cases {
        assert_eq!(jump_if_edge(value, true, &heap), truthy_edge, "JumpTrue {value:?}");
        let falsy_edge = match truthy_edge { Edge::Target => Edge::Following, Edge::Following => Edge::Target };
        assert_eq!(jump_if_edge(value, false, &heap), falsy_edge, "JumpFalse {value:?}");
    }
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![0], heap: None };
    assert!(matches!(step(&mut stage, &mut frame, Operation::Jump), Step::Control(Edge::Target)));
}

#[test]
fn a_narrow_decode_inverts_every_narrow_encoding() {
    let mut heap = Heap::new();
    let reference = heap.alloc(Cell::Error("reference".into()));
    let limit = 1_i32 << (NarrowLayout::INTEGER_BITS - 1);
    let values = [Value::number(0.0), Value::number(f64::from(limit - 1)), Value::number(f64::from(-limit)),
        Value::UNDEFINED, Value::NULL, Value::TRUE, Value::FALSE, Value::DELETED, reference];
    let mut stage = Concrete::new(std::ptr::null());
    for value in values {
        let narrow = value.to_narrow().unwrap_or_else(|| panic!("{value:?} has a narrow form"));
        let decoded = decode_narrow(&mut stage, u64::from(narrow));
        assert_eq!(Value::from_bits(decoded), Value::from_narrow(narrow), "{value:?}");
        assert_eq!(Value::from_bits(decoded), value);
    }
}

/// Hand-written decodes of the object and narrow layouts outside `staged`, per native-tier
/// file: each is a second home of a rule `staged` owns (AGENTS.md, manifesto 4). The counts
/// only go down; a new decode belongs in `staged` as an operation every tier runs.
const LAYOUT_DECODES: &[(&str, &str, usize)] = &[
    ("vm/compiled.rs", include_str!("../vm/compiled.rs"), 9),
    ("vm/compiled/lane_region.rs", include_str!("../vm/compiled/lane_region.rs"), 5),
    ("vm/compiled/osr_loop.rs", include_str!("../vm/compiled/osr_loop.rs"), 1),
    ("vm/compiled/object_slots.rs", include_str!("../vm/compiled/object_slots.rs"), 2),
    ("vm/js_compiled.rs", include_str!("../vm/js_compiled.rs"), 0),
    ("vm/dispatch_fast.rs", include_str!("../vm/dispatch_fast.rs"), 0),
];

/// The layout facts whose use is a decode: the bits a reader tests to learn a storage form or
/// a narrow value's meaning.
const LAYOUT_TOKENS: &[&str] = &[
    "extras_tag_mask",
    "inline_storage_mask",
    "storage_start_mask",
    "NarrowLayout::INTEGER_FLAG",
    "NarrowLayout::CODE_SHIFT",
];

#[test]
fn staged_coverage_layout_decodes_outside_staged_only_shrink() {
    for (file, source, allowed) in LAYOUT_DECODES {
        let found: usize = LAYOUT_TOKENS.iter().map(|token| source.matches(token).count()).sum();
        assert!(found <= *allowed,
            "{file} decodes object or narrow layouts {found} times (allowed {allowed}): route the read through `staged`");
        assert!(found >= *allowed,
            "{file} now decodes layouts {found} times, below {allowed}: lower its allowance in LAYOUT_DECODES");
    }
}

/// The interpreters: the one place an opcode's meaning is written by hand.
const INTERPRETERS: &[&str] = &["vm/dispatch.rs", "vm/dispatch_wasm.rs", "vm/dispatch_wasm/gc.rs"];

/// Opcode names per file and opcode (`Op::X` anywhere: a match arm, a comparison, a shape
/// table) in `vm/`, outside the interpreters and tests. A tier that names an opcode decides
/// something about its meaning by hand; one that reads the opcode table's facts
/// (`control_flow_layout`, `input_binding_times`, `field_store`) derives it.
fn opcode_arms() -> std::collections::BTreeMap<(String, String), usize> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut pending = vec![root.join("vm")];
    let mut arms = std::collections::BTreeMap::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap().map(Result::unwrap) {
            let path = entry.path();
            if path.is_dir() { pending.push(path); continue; }
            let file = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            let name = path.file_name().unwrap().to_string_lossy();
            if !name.ends_with(".rs") || name.ends_with("tests.rs") || INTERPRETERS.contains(&file.as_str()) { continue; }
            count_opcode_names(&std::fs::read_to_string(&path).unwrap(), &file, &mut arms);
        }
    }
    arms
}

/// Adds `source`'s opcode names, outside an inline test module, to `arms` under `file`.
fn count_opcode_names(source: &str, file: &str, arms: &mut std::collections::BTreeMap<(String, String), usize>) {
    // An inline test module closes the file; its fixtures name opcodes freely.
    let code = source.split("#[cfg(test)]\nmod tests").next().unwrap();
    for (at, _) in code.match_indices("Op::") {
        let op: String = code[at + 4..].chars().take_while(char::is_ascii_alphanumeric).collect();
        let qualified = code[..at].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
        if !qualified && op.starts_with(|c: char| c.is_ascii_uppercase()) {
            *arms.entry((file.to_owned(), op)).or_insert(0) += 1;
        }
    }
}

/// Futamura's first projection, enforced: a tier's meaning of an opcode is the interpreter's,
/// specialized (`staged`, semantic entries), so an interpreter change reaches every tier. An
/// opcode meaning a tier writes by hand is a second definition that can drift; the table of
/// them only shrinks, and a new one fails here.
#[test]
fn tiers_derive_opcode_meaning_from_the_interpreter() {
    let allowed: std::collections::BTreeMap<(String, String), usize> = include_str!("hand_semantics.txt").lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let mut fields = line.split(' ');
            let (file, op, count) = (fields.next().unwrap(), fields.next().unwrap(), fields.next().unwrap());
            ((file.to_owned(), op.to_owned()), count.parse().unwrap())
        })
        .collect();
    let found = opcode_arms();
    for ((file, op), count) in &found {
        let limit = allowed.get(&(file.clone(), op.clone())).copied().unwrap_or(0);
        assert!(*count <= limit, "{file} names Op::{op} {count} times (allowed {limit}): derive its meaning from the \
            interpreter (`staged`, a semantic entry, an opcode-table fact) instead");
    }
    for ((file, op), limit) in &allowed {
        let count = found.get(&(file.clone(), op.clone())).copied().unwrap_or(0);
        assert!(count >= *limit, "{file} now names Op::{op} {count} times: lower its line in \
            staged/hand_semantics.txt to {count} (or drop it at 0)");
    }
}

/// `++`/`--` of an int32 register, or `None` where it left at its miss edge.
fn concrete_increment(value: i32, decrement: bool, overflow: Overflow) -> Option<NumberWord> {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = number_frame(NumberWord::Int(value), NumberWord::Int(0));
    let operation = Operation::IncDec { destination: 2, source: 0, decrement, overflow };
    match step(&mut stage, &mut frame, operation) {
        Step::Value(_) => Some(frame.values[2]),
        Step::Unresolved => None,
        Step::Control(_) => panic!("an increment transfers no control"),
    }
}

#[test]
fn staged_increments_are_int32_sums_with_one() {
    assert_eq!(concrete_increment(41, false, Overflow::Exit), Some(NumberWord::Int(42)));
    assert_eq!(concrete_increment(-41, true, Overflow::Exit), Some(NumberWord::Int(-42)));
    assert_eq!(concrete_increment(i32::MAX, false, Overflow::Exit), None);
    assert_eq!(concrete_increment(i32::MIN, true, Overflow::Exit), None);
    assert_eq!(concrete_increment(i32::MAX, false, Overflow::Wrap), Some(NumberWord::Int(i32::MIN)));
    assert_eq!(concrete_increment(i32::MIN, true, Overflow::Widen),
        Some(NumberWord::Float(f64::from(i32::MIN) - 1.0)));
}
