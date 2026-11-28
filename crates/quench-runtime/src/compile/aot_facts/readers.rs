//! The reads that may observe a field's stored value, and the calls whose result no read follows.
//!
//! A field key (`FactKey::Field`, `AnyField`) that no reader observes has no reader: its stores are dead,
//! and its slot need not exist in the layout (`AotFacts::field_has_reader`). The reads are taken from each
//! instruction, not from `FunctionFlow::reads`, which records a read only where the solve kept the read's
//! origin: a read whose origin was lost (a `+=` on a field, a computed read) would otherwise be missed.
//!
//! A named read observes the fields of the classes its object may be (`FunctionFlow::field_owners`). Code the
//! solve cannot follow is a sink: an object a sink may hold is read in any way, so each class's fields it may
//! hold have a reader, and so does each class a stored field of such a class may hold (`close`). Sinks are the
//! escaped cells of the solve (`escapes::Escapes::sinks`: the operands of an unresolved call, the values a function
//! throws or yields, the values stored into a global or into an object of no one class, and the returns of an escaped
//! closure), and the operands the instructions hand on, which `instruction` notes.
//! A computed read with a key that is not a number reads its object in any way; a numeric key reads only the
//! fields named by array indices (`index_reads`).

use std::collections::{BTreeMap, BTreeSet};

use super::escapes::Escapes;
use super::flow::{Call, FunctionFlow, Target};
use super::gate;
use super::scope::Scope;
use super::shape::Change;
use super::value::{Key, Origin};
use super::AotFacts;
use crate::bytecode::{used_registers, Class, FieldLookup, Function, Op, PropertyDefinitionMode, ResidualProgram, Traits, WideInstruction};
use crate::compile::liveness::{register_dead, RegisterMasks};
use crate::staged::Operation;
use crate::typed_ir::{Cell, Classes, FactKey, Type};
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// The names builtins read off an object with no read instruction of the program (`FieldReaders::protocol_names`).
const PROTOCOL_NAMES: [&str; 33] = [
    "toString", "valueOf", "toLocaleString", "toJSON", "then", "constructor", "length", "next", "done", "value", "return",
    "throw", "lastIndex", "source", "flags", "global", "ignoreCase", "multiline", "sticky", "unicode", "unicodeSets", "dotAll",
    "hasIndices", "exec", "join", "message", "name", "cause", "index", "input", "groups", "get", "set",
];


/// The global object whose methods read none of their receiver's properties (their arguments are still sinks).
const MATH: &str = "Math";

/// The reads and escapes of a program, and the calls whose results are dead (`result_dead`). Filled by `scan`
/// per function while the facts settle, and closed by `finish` once the fact cells are settled.
pub(super) struct Readers {
    /// The atoms named by an array index (`"0"`, `"7"`): a numeric computed read reads these.
    index_atoms: BTreeSet<u32>,
    /// The atoms named `Math` (the receiver `Math.f()` takes no escape).
    math: BTreeSet<u32>,
    /// Per named atom, the classes of the objects it is read off by name; `Never` where none holds it.
    named: BTreeMap<u32, Classes>,
    /// The atoms read by name off some object, of any class.
    named_seen: BTreeSet<u32>,
    /// The classes of the objects a numeric computed read reads.
    index_reads: Classes,
    /// The cells code the solve cannot follow may hold: an object in one is read in any way.
    sinks: Vec<Cell>,
    /// The method calls `finish` resolves per class of their receiver.
    methods: Vec<MethodCall>,
    /// The classes whose objects code the solve cannot follow may read, field by field.
    reflective: Classes,
    /// `(function, pc)` of each call whose result register no read follows.
    dead_results: BTreeSet<(u32, u32)>,
    /// Whether a builtin may make a field non-writable or the object non-extensible under a name the program does not name
    /// (a `defineProperty` with an unknown key, `freeze`, `Proxy`, ...): a store of any field then may fail instead of writing,
    /// so every field's stores are observed (`observes`).
    descriptors_unknown: bool,
}

/// A method call `receiver.atom(arguments)`, which escapes its operands unless `resolves` names its callees.
struct MethodCall {
    atom: u32,
    receiver: Cell,
    arguments: Vec<Cell>,
}

/// The flow and facts one function's scan reads.
struct Scan<'a> {
    program: &'a ResidualProgram,
    function: u32,
    flow: &'a FunctionFlow,
    facts: &'a AotFacts,
    scope: &'a Scope,
    masks: Option<&'a RegisterMasks>,
    calls: BTreeMap<u32, &'a Call>,
    /// Whether the program names `defineProperty` (`gate::names_property_definer`): a callee read through no name may be it.
    definer: bool,
}

impl Scan<'_> {
    /// The cell `register` holds just before the instruction at `at` (`RegisterCells::at`); none where no path reaches it.
    fn cell(&self, at: u32, register: u16) -> Cell {
        self.flow.registers.at(at, register).unwrap_or(Cell::NEVER)
    }
}

impl Readers {
    /// No read yet, over the atoms of `program`.
    pub(super) fn new(program: &ResidualProgram) -> Self {
        Self {
            index_atoms: atoms_named(program, is_index_name),
            math: atoms_named(program, |name| name == MATH),
            named: BTreeMap::new(),
            named_seen: BTreeSet::new(),
            index_reads: Classes::Never,
            sinks: Vec::new(),
            methods: Vec::new(),
            reflective: Classes::Never,
            dead_results: BTreeSet::new(),
            descriptors_unknown: false,
        }
    }

    /// Notes what function `function` reads, hands on and discards, from its flow and its instructions.
    pub(super) fn scan(&mut self, program: &ResidualProgram, function: u32, flow: &FunctionFlow, facts: &AotFacts, scope: &Scope) {
        let cx = Scan {
            program,
            definer: gate::names_property_definer(program),
            function,
            flow,
            facts,
            scope,
            masks: program.register_root_masks(function),
            // A field access is no call here (`Target::Accessor`): its instruction keeps the reads and escapes of a field access.
            calls: flow.calls.iter().filter(|call| !matches!(call.target, Target::Accessor { .. })).map(|call| (call.pc, call)).collect(),
        };
        program.with_record(function as usize, |code| {
            for pc in 0..code.code.len() {
                self.instruction(&cx, code, pc);
            }
        });
    }

    /// Settles what the scans left open, once the fact cells are settled (`cells`): a method call whose receiver's
    /// classes name their own closures escapes nothing, any other escapes its operands; then the classes a reflective
    /// read may reach: the sinks of this scan and the escaped cells of the solve (`escaped`).
    pub(super) fn finish(&mut self, cells: &BTreeMap<FactKey, Cell>, escaped: &Escapes) {
        for call in std::mem::take(&mut self.methods) {
            if !resolves(&call, cells) {
                self.sinks.push(call.receiver);
                self.sinks.extend(call.arguments);
            }
        }
        let reflective = self.sinks.iter().copied().chain(escaped.sinks.iter().copied())
            .fold(Classes::Never, |classes, cell| classes.join(holders(cell)));
        self.reflective = reflective;
        self.close(cells);
    }

    /// Notes that a store of each name in `names` (`accessors::names_of`, the names an accessor defines) may run its
    /// setter, which is guest code that may read the stored field, so the name is read off any class. Called before `finish`.
    pub(super) fn accessor_names(&mut self, names: &BTreeSet<u32>) {
        for &atom in names {
            self.named(atom, Classes::Many);
        }
    }

    /// Notes that each protocol name the program names (`PROTOCOL_NAMES`) is read off any class: a builtin reads it with no
    /// read instruction the flows record (a conversion's `toString`/`valueOf`, `await`'s `then`, `JSON.stringify`'s `toJSON`,
    /// the iterator protocol, a species `constructor`, a regexp's `lastIndex`, an error's `message`), so a store of it is
    /// observed. Called before `finish`.
    pub(super) fn protocol_names(&mut self, program: &ResidualProgram) {
        for (atom, name) in program.atoms.iter().enumerate() {
            let name: &str = name.as_ref();
            if PROTOCOL_NAMES.contains(&name) {
                self.named(atom as u32, Classes::Many);
            }
        }
    }

    /// Whether a read the program makes, or code it cannot follow makes, may observe the value stored under `key`.
    /// Only a field key is decided here; any other key is observed (`true`). Where `descriptors_unknown` holds, every field
    /// key is observed: a store of a field some builtin may have made non-writable, or may fail on a non-extensible object,
    /// must not be dropped, since dropping it would turn a failing store into a silent one.
    pub(super) fn observes(&self, key: FactKey) -> bool {
        match key {
            FactKey::Field { .. } | FactKey::AnyField(_) if self.descriptors_unknown => true,
            FactKey::Field { class, atom, .. } => self.reflective.may_hold(class)
                || self.named.get(&atom).is_some_and(|classes| classes.may_hold(class))
                || (self.index_atoms.contains(&atom) && self.index_reads.may_hold(class)),
            FactKey::AnyField(atom) => self.named_seen.contains(&atom)
                || self.reflective != Classes::Never
                || (self.index_atoms.contains(&atom) && self.index_reads != Classes::Never),
            _ => true,
        }
    }

    /// Whether code the solve cannot follow may enumerate or read every field of an instance of `class`: an instance reaches a
    /// sink (`reflective`), where a for-in, a spread, `Object.keys`, a `JSON` walk or `Reflect.ownKeys` is not followed. Those
    /// operations hand their operands to a sink (`plain`, `open`), so they are the enumerators; no name table is consulted.
    pub(super) fn reflects_class(&self, class: u32) -> bool {
        self.reflective.may_hold(class)
    }

    /// Whether the call at `pc` of function `function` has a result no read follows (`call` in `scan`).
    pub(super) fn result_dead(&self, function: u32, pc: u32) -> bool {
        self.dead_results.contains(&(function, pc))
    }

    /// Notes the reads, escapes and discarded results of one instruction at `pc`.
    fn instruction(&mut self, cx: &Scan<'_>, code: &Function, pc: usize) {
        let at = pc as u32;
        let instruction = code.instruction_at(pc);
        if instruction.op().traits().has(Traits::NAME_LOAD) {
            self.named(instruction.atom_index(), Classes::Many);
        }
        if let Some(call) = cx.calls.get(&at) {
            self.descriptors(cx, call);
            self.call(cx, call);
            if discards_result(cx.masks, instruction, at) {
                self.dead_results.insert((cx.function, at));
            }
        } else if instruction.op().is_return() && !cx.flow.returns_out {
            // The value of a return the solve follows is its function's `Key::Return` write, which `escapes` holds where the
            // function escapes, so it is no sink here.
        } else {
            self.operation(cx, at, instruction);
        }
    }

    /// Notes what a non-call instruction reads: a named read, a computed read, or a store whose key or object may reach
    /// code the solve cannot follow. Every other instruction moves values, or reads none that reaches a field.
    fn operation(&mut self, cx: &Scan<'_>, at: u32, instruction: WideInstruction) {
        if instruction.op().class() == Class::GetField {
            self.field_read(cx, at, instruction);
        } else if instruction.op() == Op::GetIndex {
            self.element_read(cx, at, instruction);
        } else {
            match Operation::<u64, ()>::of(instruction) {
                Some(Operation::SetField { value, .. }) if owner_unknown(cx, at) => self.sinks.push(cx.cell(at, u16::from(value))),
                Some(Operation::SetField { .. }) => {}
                // A computed key that is an object is converted to a name: its `toString` runs.
                Some(Operation::ElementWrite { key, .. }) => self.sinks.push(cx.cell(at, u16::from(key))),
                _ => self.plain(cx, at, instruction),
            }
        }
    }

    /// Notes an instruction the cases above leave: the object literal and move instructions read no operand, a
    /// definition converts its computed key, and every other instruction hands its operands on.
    fn plain(&mut self, cx: &Scan<'_>, at: u32, instruction: WideInstruction) {
        match instruction.op().class() {
            Class::DefinePropertyRecord => {
                if matches!(instruction.property_definition_mode(), Some(PropertyDefinitionMode::ReadonlyMethod)) {
                    self.readonly_definition(cx, at);
                }
                self.sinks.push(cx.cell(at, u16::from(instruction.register_c())));
            }
            class if moves_only(class) => {}
            _ => {
                for register in used_registers(instruction) {
                    self.sinks.push(cx.cell(at, register));
                }
            }
        }
    }

    /// Notes a definition of a non-writable method (`PropertyDefinitionMode::ReadonlyMethod`, a class's private method) at `at`:
    /// a store of the name it defines fails instead of writing, so the name is observed off any class. A key the shape does
    /// not name is any name (`descriptors_unknown`).
    fn readonly_definition(&mut self, cx: &Scan<'_>, at: u32) {
        match cx.flow.shapes.iter().find(|shape| shape.pc == at).map(|shape| &shape.change) {
            Some(Change::Field(atom)) => self.named(*atom, Classes::Many),
            _ => self.descriptors_unknown = true,
        }
    }

    /// Notes what a call does to the descriptors of the objects it is given. A `defineProperty` with a known key may make that
    /// name non-writable or an accessor, so the name is observed off any class; with an unknown key it may make any name one,
    /// and any other builtin that defines properties or makes an object non-writable or non-extensible (`Scope::changes_descriptors`)
    /// makes every field observed (`descriptors_unknown`).
    fn descriptors(&mut self, cx: &Scan<'_>, call: &Call) {
        if gate::is_property_definer(cx.program, call, cx.definer) {
            match call.arguments.get(1).and_then(|key| key.atom_key()) {
                Some(atom) => self.named(atom, Classes::Many),
                None => self.descriptors_unknown = true,
            }
        } else if callee_atom(call).is_some_and(|atom| cx.scope.changes_descriptors(atom, call.arguments.len())) {
            self.descriptors_unknown = true;
        }
    }

    /// Notes a named field read: of the classes its object may be (`field_owners`), or of any object where the lookup
    /// is a path (`Site`) whose intermediate value's classes are unknown.
    fn field_read(&mut self, cx: &Scan<'_>, at: u32, instruction: WideInstruction) {
        let owner = cx.flow.field_owners.get(&at).copied().unwrap_or(Classes::Many);
        match instruction.field_lookup() {
            Some(FieldLookup::Atom { atom, .. }) => self.named(atom, owner),
            Some(FieldLookup::Site(index)) => match cx.scope.site_paths.get(index).copied().flatten() {
                Some((_, first, None)) => self.named(first, owner),
                Some((_, first, Some(second))) => {
                    self.named(first, Classes::Many);
                    self.named(second, owner);
                }
                None => self.sinks.push(Cell::ANY),
            },
            None => self.sinks.push(Cell::ANY),
        }
    }

    /// Notes a computed read `base[key]`: a numeric key reads the array-index fields of `base`, any other key reads
    /// `base` in any way. An object key is converted to a name first, so it is a sink too.
    fn element_read(&mut self, cx: &Scan<'_>, at: u32, instruction: WideInstruction) {
        let base = cx.cell(at, u16::from(instruction.register_b()));
        let key = cx.cell(at, u16::from(instruction.register_c()));
        self.sinks.push(key);
        if is_number(key) {
            self.index_reads = self.index_reads.join(holders(base));
        } else {
            self.sinks.push(base);
        }
    }

    /// Records that the program reads `atom` by name off objects of `holders`.
    fn named(&mut self, atom: u32, holders: Classes) {
        let slot = self.named.entry(atom).or_insert(Classes::Never);
        *slot = slot.join(holders);
        self.named_seen.insert(atom);
    }

    /// Notes a call: a method read names its atom on the receiver, then the callee's operands escape unless the callee
    /// is one the solve proves (a known function, a proven closure, a construction of a proven class).
    fn call(&mut self, cx: &Scan<'_>, call: &Call) {
        let key = (cx.function, call.pc);
        if call.construct {
            if !cx.facts.construct_targets.contains_key(&key) {
                self.open(call);
            }
            return;
        }
        match call.target {
            Target::Known(_) => {}
            Target::Register if proven(cx, key) => {}
            Target::Register => self.open(call),
            Target::Method(atom) => self.method(cx, call, atom),
            // Left out of `Scan::calls` (`scan`): an accessor access is read as the field access it is.
            Target::Accessor { .. } => {}
        }
    }

    /// Notes a method call `receiver.atom(...)`: the array methods read no operand, a `Math` receiver's arguments
    /// escape alone, and any other call is resolved by `finish`.
    fn method(&mut self, cx: &Scan<'_>, call: &Call, atom: u32) {
        self.named(atom, call.receiver.cell.classes);
        if cx.scope.is_array_builtin(atom) {
            return;
        }
        let arguments = call.arguments.iter().map(|word| word.cell);
        if self.is_math(call) {
            self.sinks.extend(arguments);
        } else {
            self.methods.push(MethodCall { atom, receiver: call.receiver.cell, arguments: arguments.collect() });
        }
    }

    /// Escapes the operands of a call the solve does not resolve: its callee, its receiver (unless `Math`), its arguments.
    fn open(&mut self, call: &Call) {
        if call.target == Target::Register {
            self.sinks.push(call.callee.cell);
        }
        if !self.is_math(call) {
            self.sinks.push(call.receiver.cell);
        }
        self.sinks.extend(call.arguments.iter().map(|word| word.cell));
    }

    /// Whether the call's receiver is the global `Math` object.
    fn is_math(&self, call: &Call) -> bool {
        matches!(call.receiver.origin, Some(Origin::Binding(Key::Global(atom))) if self.math.contains(&atom))
    }

    /// Adds to `reflective` the classes a reflective field of a reflective class, or any reflective object's
    /// `AnyField`, may hold, to a fixed point.
    fn close(&mut self, cells: &BTreeMap<FactKey, Cell>) {
        loop {
            let before = self.reflective;
            for (key, cell) in cells {
                if self.reflects(*key) {
                    self.reflective = self.reflective.join(holders(*cell));
                }
            }
            if self.reflective == before {
                return;
            }
        }
    }

    /// Whether a reflective read of an object may read the value stored under `key`.
    fn reflects(&self, key: FactKey) -> bool {
        match key {
            FactKey::Field { class, .. } => self.reflective.may_hold(class),
            FactKey::AnyField(_) => self.reflective != Classes::Never,
            _ => false,
        }
    }
}

/// The atom of the name `call` is read by: its method's, or the name of its callee read off a named property or a global.
pub(super) fn callee_atom(call: &Call) -> Option<u32> {
    match (call.target, call.callee.origin) {
        (Target::Method(atom), _) => Some(atom),
        (_, Some(Origin::Member(atom) | Origin::MemberOn { atom, .. } | Origin::Binding(Key::Global(atom)))) => Some(atom),
        _ => None,
    }
}

/// Whether `call` resolves: the receiver holds a closed set of classes, each with a method of that name that is a closure.
fn resolves(call: &MethodCall, cells: &BTreeMap<FactKey, Cell>) -> bool {
    matches!(call.receiver.classes, Classes::Set { .. }) && call.receiver.classes.ids().iter().all(|&class| {
        cells.get(&FactKey::Field { class, instance: true, atom: call.atom }).is_some_and(|cell| cell.ty.body_index().is_some())
    })
}

/// Whether a call the scan saw is proven: its target is the one the solve found, not a guess or a closed set of candidates.
fn proven(cx: &Scan<'_>, key: (u32, u32)) -> bool {
    cx.facts.call_targets.contains_key(&key)
        && !cx.facts.guessed_calls.contains(&key)
        && !cx.facts.call_candidates.contains_key(&key)
}

/// Whether the call `instruction` at `pc` writes a result register that no read follows (liveness after `pc`). A call that
/// also sets a flag beside its result keeps it.
fn discards_result(masks: Option<&RegisterMasks>, instruction: WideInstruction, pc: u32) -> bool {
    match (masks, instruction.defined_register()) {
        (Some(masks), Some(register)) => instruction.result_flags() == 0
            && masks.get(pc as usize + 1).is_some_and(|after| register_dead(after, usize::from(register))),
        _ => false,
    }
}

/// Whether the object `at` reads or stores through is of no one class the solve names (or is not known): a store into it
/// may land in a global, which the host reads by name.
fn owner_unknown(cx: &Scan<'_>, at: u32) -> bool {
    cx.flow.field_owners.get(&at).map_or(true, |classes| *classes == Classes::Many)
}

/// The classes the objects a cell holds may be: a function's own class where its type names one (its static fields),
/// else the cell's classes.
fn holders(cell: Cell) -> Classes {
    cell.ty.body_index().map_or(cell.classes, |function| Classes::one(function, false))
}

/// Whether a cell holds only Numbers.
fn is_number(cell: Cell) -> bool {
    cell.ty != Type::NEVER && cell.ty.is_subtype(Type::NUMBER)
}

/// Whether an instruction of class `class` only moves values between registers, bindings, environments and fresh
/// allocations, and reads no operand's property.
fn moves_only(class: Class) -> bool {
    matches!(class,
        Class::Move | Class::LoadConst | Class::Nop | Class::Jump | Class::JumpTrue | Class::JumpFalse
        | Class::LoadThis | Class::LoadCapture | Class::LoadLocal | Class::LoadLocalPlain | Class::LoadEnvLocal
        | Class::StoreLocal | Class::StoreLocalPlain | Class::StoreEnvLocal | Class::StoreCapture | Class::CloneEnv
        | Class::DefineArrayElement | Class::MakeArray | Class::MakeObject | Class::MakeObject2
        | Class::MakeObjectLiteral | Class::MakeConstArray | Class::MakeClosure)
}

/// Whether `name` is an array index: its canonical decimal form, below 2^32 - 1.
fn is_index_name(name: &str) -> bool {
    name.parse::<u32>().is_ok_and(|index| index != u32::MAX && index.to_string() == name)
}

/// The atoms of `program` whose names `keep` accepts.
fn atoms_named(program: &ResidualProgram, keep: impl Fn(&str) -> bool) -> BTreeSet<u32> {
    program.atoms.iter().enumerate().filter_map(|(atom, name)| keep(name.as_ref()).then_some(atom as u32)).collect()
}

#[cfg(test)]
mod tests {
    use super::super::{analyze, AotFacts};
    use crate::bytecode::ResidualProgram;
    use crate::typed_ir::FactKey;
    use crate::Engine;

    fn program(source: &str) -> ResidualProgram {
        Engine::specialize(source, "readers.js").unwrap()
    }

    fn atom(program: &ResidualProgram, name: &str) -> u32 {
        program.atoms.iter().position(|atom| atom == name).expect("an atom of that name") as u32
    }

    /// The fact keys the settled cells hold for field `atom`, of any class.
    fn keys_of(facts: &AotFacts, atom: u32) -> Vec<FactKey> {
        facts.cells.keys().copied().filter(|key| match *key {
            FactKey::Field { atom: name, .. } | FactKey::AnyField(name) => name == atom,
            _ => false,
        }).collect()
    }

    /// The id of the function named `name`.
    fn function_named(program: &ResidualProgram, name: &str) -> u32 {
        let named = |id: usize| program.with_record(id, |function| function.name.is_some_and(|atom| {
            let text: &str = program.atoms[atom as usize].as_ref();
            text == name
        }));
        (0..program.function_count()).find(|&id| named(id)).expect("a function with that name") as u32
    }

    /// The pc of the call `caller` makes to `callee`.
    fn call_pc(facts: &AotFacts, caller: u32, callee: u32) -> u32 {
        facts.call_targets.iter().find(|&(&(function, _), &target)| function == caller && target == callee)
            .map(|(&(_, pc), _)| pc)
            .expect("a call of that callee")
    }

    #[test]
    fn a_field_only_written_has_no_reader() {
        let program = program("(function () { function T() { this.dead = 1; this.live = 2; } return new T().live; })();");
        let facts = analyze(&program);
        let dead = keys_of(&facts, atom(&program, "dead"));
        assert!(!dead.is_empty(), "the store settles a cell");
        assert!(dead.iter().all(|&key| !facts.field_has_reader(key)));
        let live = keys_of(&facts, atom(&program, "live"));
        assert!(live.iter().all(|&key| facts.field_has_reader(key)));
    }

    #[test]
    fn a_name_an_accessor_defines_has_a_reader() {
        let program = program("(function () { function T() { this.dead = 1; } var o = { set dead(v) {} }; o.dead = 2; return new T(); })();");
        let facts = analyze(&program);
        let keys = keys_of(&facts, atom(&program, "dead"));
        assert!(!keys.is_empty(), "the store settles a cell");
        assert!(keys.iter().all(|&key| facts.field_has_reader(key)), "a store of the name may run the setter");
    }

    #[test]
    fn a_computed_read_with_an_unknown_key_has_a_reader() {
        let program = program("(function (k) { function T() { this.x = 1; } var t = new T(); return t[k]; })('x');");
        let facts = analyze(&program);
        let keys = keys_of(&facts, atom(&program, "x"));
        assert!(!keys.is_empty(), "the store settles a cell");
        assert!(keys.iter().all(|&key| facts.field_has_reader(key)));
    }

    #[test]
    fn a_call_whose_result_nothing_reads_is_unused() {
        let program = program("(function () { function f() { return 1; } function g() { f(); return 2; } \
            function h() { return f(); } g(); h(); })();");
        let facts = analyze(&program);
        let (f, g, h) = (function_named(&program, "f"), function_named(&program, "g"), function_named(&program, "h"));
        assert!(!facts.function_facts(g).result_used(call_pc(&facts, g, f)), "g discards f's result");
        assert!(facts.function_facts(h).result_used(call_pc(&facts, h, f)), "h returns f's result");
    }
}
