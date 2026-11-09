use super::*;
use crate::bytecode::Operand;
use crate::heap::{Cell, Heap, Object};
use crate::value_vec::INLINE_PROPERTY_COUNT;
use crate::Value;
use crate::value::NarrowLayout;
use super::object::decode_narrow;

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

    /// The test frame's activation holds no `this`.
    fn this(&mut self, _stage: &mut Concrete) -> u64 { Value::DELETED.to_bits() }

    fn define_this(&mut self, _stage: &mut Concrete, destination: Register, this: u64) {
        self.registers[usize::from(destination)] = this;
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

    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) {
        (Value::boolean(condition).to_bits(), ())
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
    fn prove_number(&mut self, stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        Value::from_bits(self.read(stage, register)).as_number().map(|_| ())
    }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
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

/// Logical negation is ToBoolean then the canonical boolean: one operation for the
/// interpreter and every tier.
#[test]
fn staged_logical_not_is_the_javascript_truthiness_negation() {
    let mut stage = Concrete::new(std::ptr::null());
    for (value, expected) in [
        (Value::TRUE, false), (Value::FALSE, true), (Value::number(0.0), true),
        (Value::number(-0.0), true), (Value::number(f64::NAN), true),
        (Value::number(3.0), false), (Value::NULL, true), (Value::UNDEFINED, true),
    ] {
        let mut frame = ConcreteFrame { registers: vec![value.to_bits(), 0], heap: None };
        let Step::Value(word) = step(&mut stage, &mut frame,
            Operation::LogicalNot { destination: 1, source: 0 }) else {
            panic!("logical not produces a value")
        };
        assert_eq!(Value::from_bits(word), Value::boolean(expected), "{value:?}");
        assert_eq!(Value::from_bits(frame.registers[1]), Value::boolean(expected));
    }
}

/// The catalog's staged set and `Operation::decode` agree: every opcode the catalog marks
/// `Staged` decodes to one operation, and every other opcode is explicitly `Exact`.
#[test]
fn every_staged_opcode_decodes_to_one_operation() {
    use crate::bytecode::{Op, Semantics, WideInstruction};
    let constants = [Constant::untyped(Value::number(1.0))];
    let staged = [Op::Move, Op::LoadConst, Op::LoadLocalPlain, Op::StoreLocalPlain,
        Op::Jump, Op::JumpFalse, Op::JumpTrue, Op::LoadThis, Op::RequireObjectCoercible,
        Op::ToNumeric];
    for op in staged {
        assert_eq!(op.semantics(), Semantics::Staged, "{op:?}");
        let instruction = WideInstruction::new(op, 1, 2, 3, 0);
        assert!(Operation::decode(instruction, |index| constants.get(index).copied()).is_some(),
            "{op:?} must decode to a staged operation");
    }
    for index in 0..Op::COUNT {
        let op = Op::from_index(index).unwrap();
        if !staged.contains(&op) {
            assert_eq!(op.semantics(), Semantics::Exact, "{op:?} must be an explicit exact path");
        }
    }
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
    ("vm/compiled/osr_loop.rs", include_str!("../vm/compiled/osr_loop.rs"), 0),
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
        // `WideInstruction::new(Op::X, ...)` supplies an opcode value to the builder; it is
        // not a second definition of the opcode's meaning.
        if code[..at].trim_end().ends_with("WideInstruction::new(") { continue; }
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

#[test]
fn staged_require_object_coercible_rejects_only_null_and_undefined() {
    let coercible = |value: Value| {
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = ConcreteFrame { registers: vec![value.to_bits()], heap: None };
        match step(&mut stage, &mut frame, Operation::RequireObjectCoercible { register: 0 }) {
            Step::Value(word) => { assert_eq!(word, value.to_bits()); true }
            Step::Unresolved => false,
            Step::Control(_) => panic!("a coercibility test transfers no control"),
        }
    };
    assert!(!coercible(Value::NULL));
    assert!(!coercible(Value::UNDEFINED));
    for value in [Value::TRUE, Value::FALSE, Value::number(0.0), Value::number(f64::NAN), Value::number(-0.0)] {
        assert!(coercible(value), "{value:?}");
    }
}

#[test]
fn staged_to_numeric_copies_numbers_and_leaves_for_the_rest() {
    let convert = |value: Value| {
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = ConcreteFrame { registers: vec![value.to_bits(), Value::UNDEFINED.to_bits()], heap: None };
        match step(&mut stage, &mut frame, Operation::Number(NumberOperation::ToNumeric { destination: 1, source: 0 })) {
            Step::Value(_) => Some(Value::from_bits(frame.registers[1])),
            Step::Unresolved => { assert_eq!(frame.registers[1], Value::UNDEFINED.to_bits()); None }
            Step::Control(_) => panic!("a conversion transfers no control"),
        }
    };
    for value in [Value::number(1.5), Value::number(-0.0), Value::number(f64::NAN), Value::number(7.0)] {
        assert_eq!(convert(value).map(Value::to_bits), Some(value.to_bits()), "{value:?}");
    }
    for value in [Value::NULL, Value::UNDEFINED, Value::TRUE] {
        assert_eq!(convert(value), None, "{value:?}");
    }
}

#[test]
fn staged_load_this_leaves_where_the_activation_holds_no_this() {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = ConcreteFrame { registers: vec![Value::NULL.to_bits()], heap: None };
    assert!(matches!(step(&mut stage, &mut frame, Operation::LoadThis { destination: 0 }), Step::Unresolved));
    assert_eq!(frame.registers[0], Value::NULL.to_bits(), "a load that left writes nothing");
}
