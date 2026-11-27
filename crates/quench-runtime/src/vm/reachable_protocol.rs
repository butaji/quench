//! The protocol gates of the closed world: the string names and well-known symbols a runtime
//! protocol reads on a value the program did not name. An opcode or a native opens the gate of the
//! protocol it runs; a closed gate keeps its keys out of the closure. Opening a gate revisits what
//! the closure has already reached, since a reached object may expose the key the gate now admits.
//! A computed key that is proven a Number names only numeric keys; any other computed key may name
//! every key (`wild`).

use super::*;
use super::native_effects::NativeEffects;
use super::reachable_realm::Closure;
use crate::bytecode::{Op, Operand, OperandKind, Register, WideInstruction};
use crate::compile::aot_facts::FunctionFacts;
use crate::operator::{BinaryOperator, UnaryOperator};
use crate::typed_ir::Type;
use crate::ValueType;

/// The well-known symbols whose keys are followed only once their protocol is open.
const GATED_SYMBOLS: [WellKnownSymbol; 4] = [
    WellKnownSymbol::ToPrimitive, WellKnownSymbol::Iterator, WellKnownSymbol::AsyncIterator, WellKnownSymbol::HasInstance,
];

/// The names an opcode's protocol reads implicitly: the thrown value's report and a thenable check.
pub(super) fn protocol_names(op: Op) -> &'static [&'static str] {
    match op {
        Op::Throw => &["message", "stack", "name"],
        Op::Await | Op::Yield | Op::YieldStar | Op::GetAsyncIterator => &["then"],
        _ => &[],
    }
}

/// Opcodes whose property key is computed at run time.
fn computed_key_op(op: Op) -> bool {
    matches!(
        op,
        Op::GetIndex | Op::SetIndex | Op::ToPropertyKey | Op::DefineComputedField | Op::ForInStart | Op::ForInNext
            | Op::GetEnumerated | Op::CopyDataProperties | Op::Delete
    )
}

/// Whether `op` writes a named field of its base: its key is a store's, not a read's.
pub(super) fn writes_field(op: Op) -> bool {
    matches!(op, Op::SetField | Op::SetFieldStrict | Op::SetThisField | Op::SetThisFieldStrict | Op::DefineField)
}

/// Revisits every reached object, so the keys a newly opened gate admits are read.
pub(super) fn requeue(closure: &mut Closure) {
    closure.pending.extend(closure.seen.iter().copied());
}

/// Visits `value` again although it is reached: its receiver names have just changed.
fn revisit(closure: &mut Closure, value: Value) {
    closure.seen.insert(value);
    closure.pending.push(value);
}

/// Opens the global name `atom` for every reached object.
pub(super) fn open_name(closure: &mut Closure, atom: Atom) {
    if closure.names.insert(atom) {
        requeue(closure);
    }
}

/// Makes every string key of every reached object readable.
pub(super) fn make_wild(closure: &mut Closure) {
    if !closure.wild {
        closure.wild = true;
        requeue(closure);
    }
}

/// Admits the keys a computed-key access of `instruction` at `pc` may name: every key, unless its
/// key register is proven a Number, which names only numeric keys.
pub(super) fn admit_computed_key(closure: &mut Closure, facts: &FunctionFacts<'_>, pc: u32, instruction: WideInstruction) {
    let op = instruction.op();
    if !computed_key_op(op) {
        return;
    }
    let number_key = matches!(op, Op::GetIndex | Op::SetIndex)
        && facts.cell_at(pc, instruction.register_c()).is_some_and(|cell| cell.ty.is_subtype(Type::NUMBER));
    if number_key {
        if !closure.indexed {
            closure.indexed = true;
            requeue(closure);
        }
    } else {
        make_wild(closure);
    }
}

/// Whether a register may hold a function before the instruction at `pc`, by its type fact. A
/// register with no settled fact may hold anything.
fn register_may_be_function(facts: &FunctionFacts<'_>, pc: u32, register: Register) -> bool {
    facts.cell_at(pc, register).is_none_or(|cell| cell.ty.may_be_languages(Type::language(ValueType::Function)))
}

/// Whether an operand may be a function: a constant is a primitive, a register follows its fact, and
/// any other operand kind is not decoded here.
fn operand_may_be_function(facts: &FunctionFacts<'_>, pc: u32, operand: Operand) -> bool {
    match operand.kind() {
        Some(OperandKind::Constant) => false,
        _ => operand.register_index().is_none_or(|register| register_may_be_function(facts, pc, register)),
    }
}

/// Whether the operands an instruction coerces may be a function. An instruction whose operands are
/// not decoded here may coerce a function.
fn coerced_operand_may_be_function(facts: &FunctionFacts<'_>, pc: u32, instruction: WideInstruction) -> bool {
    match instruction.op() {
        Op::Binary | Op::NumericAdd | Op::NumericMultiply => [instruction.operand_b(), instruction.operand_c()]
            .into_iter()
            .any(|operand| operand_may_be_function(facts, pc, operand)),
        Op::Unary | Op::ToNumeric | Op::IncDec => register_may_be_function(facts, pc, instruction.register_b()),
        Op::GetIndex | Op::SetIndex => register_may_be_function(facts, pc, instruction.register_c()),
        _ => true,
    }
}

/// Whether a binary operator coerces an operand: every one but the strict comparisons and `instanceof`.
fn binary_coerces(operator: u32) -> bool {
    operator != BinaryOperator::StrictEquality as u32 && operator != BinaryOperator::StrictInequality as u32
        && operator != BinaryOperator::Instanceof as u32
}

/// Whether a unary operator coerces its operand: the numeric operators.
fn unary_coerces(operator: u32) -> bool {
    [UnaryOperator::UnaryPlus, UnaryOperator::UnaryNegation, UnaryOperator::BitwiseNot]
        .iter()
        .any(|coerces| *coerces as u32 == operator)
}

/// Whether a property name is numeric (a superset of the canonical numeric strings a Number key makes).
pub(super) fn numeric_name(text: &str) -> bool {
    text.parse::<f64>().is_ok()
}

impl Vm {
    /// Opens the protocols one instruction runs on its operands, with the operand type facts of the
    /// function it belongs to (`facts`, at `pc`).
    pub(super) fn open_instruction(&mut self, closure: &mut Closure, facts: &FunctionFacts<'_>, pc: u32, instruction: WideInstruction) {
        match instruction.op() {
            Op::Binary => {
                let operator = instruction.binary_operator();
                if operator == BinaryOperator::Instanceof as u32 {
                    open_name(closure, self.prototype_atom);
                    self.open_well_known(closure, WellKnownSymbol::HasInstance);
                } else if binary_coerces(operator) {
                    self.open_coerced(closure, facts, pc, instruction);
                }
            }
            Op::Unary if unary_coerces(instruction.unary_operator()) => self.open_coerced(closure, facts, pc, instruction),
            op => self.open_op(closure, facts, pc, instruction, op),
        }
    }

    /// Opens the protocols of a non-operator opcode: iteration, construction and key coercion.
    fn open_op(&mut self, closure: &mut Closure, facts: &FunctionFacts<'_>, pc: u32, instruction: WideInstruction, op: Op) {
        match op {
            Op::GetIterator | Op::GetAsyncIterator | Op::IteratorClose | Op::IteratorCleanupPush | Op::IteratorCleanupPop
            | Op::YieldStar | Op::AppendArray | Op::CallSpread => self.open_iteration(closure),
            Op::Construct | Op::ValidateClassHeritage => open_name(closure, self.prototype_atom),
            Op::NumericAdd | Op::NumericMultiply | Op::ToNumeric | Op::IncDec | Op::StringConcat | Op::ToPropertyKey
            | Op::SetFunctionNameKey | Op::Delete | Op::GetIndex | Op::SetIndex | Op::DefineComputedField => {
                self.open_coerced(closure, facts, pc, instruction)
            }
            _ => {}
        }
    }

    /// Opens the coercion protocol for an instruction that coerces its operands. The function
    /// prototype's `toString` opens only if an operand may be a function.
    fn open_coerced(&self, closure: &mut Closure, facts: &FunctionFacts<'_>, pc: u32, instruction: WideInstruction) {
        self.open_coercion(closure);
        if coerced_operand_may_be_function(facts, pc, instruction) {
            closure.function_coercion = true;
        }
    }

    /// Opens the coercion protocol: `valueOf`, `toString` and `@@toPrimitive` are read on the
    /// objects a coercion reaches.
    pub(super) fn open_coercion(&self, closure: &mut Closure) {
        if !closure.coercion {
            closure.coercion = true;
            requeue(closure);
            self.open_well_known(closure, WellKnownSymbol::ToPrimitive);
        }
    }

    /// Opens the iteration protocol: `next`, `return` and `throw` are read on the iterator
    /// receivers, and `@@iterator` and `@@asyncIterator` are followed.
    pub(super) fn open_iteration(&mut self, closure: &mut Closure) {
        if closure.iteration {
            return;
        }
        closure.iteration = true;
        let atoms = [self.runtime_atoms.next, self.runtime_atoms.iterator_return, self.intern_atom("throw")];
        for receiver in self.iterator_receivers() {
            closure.receiver_names.entry(receiver).or_default().extend(atoms);
            revisit(closure, receiver);
        }
        self.open_well_known(closure, WellKnownSymbol::Iterator);
        self.open_well_known(closure, WellKnownSymbol::AsyncIterator);
    }

    /// Opens the protocols a native runs, by its effects (`native_effects`).
    pub(super) fn open_native(&mut self, closure: &mut Closure, effects: NativeEffects) {
        if effects.coerces_args {
            self.open_coercion(closure);
            // The arguments of a coercing native are not typed here (`[f].join()` converts a function
            // element), so a function may be coerced whatever the program's operands show.
            if !closure.function_coercion {
                closure.function_coercion = true;
                requeue(closure);
            }
        }
        if effects.iterates {
            self.open_iteration(closure);
        }
        if effects.constructs {
            open_name(closure, self.prototype_atom);
        }
        if effects.resolves {
            open_name(closure, self.runtime_atoms.then);
        }
    }

    fn open_well_known(&self, closure: &mut Closure, symbol: WellKnownSymbol) {
        if let Some(value) = self.well_known_symbols.get(symbol) {
            if closure.symbols.insert(value) {
                requeue(closure);
            }
        }
    }

    /// Whether the closure exposes `key` of the reached object `holder`: a string key the program or
    /// its runtime can name, a key of an open protocol, or any string key once `wild`. A gated
    /// well-known symbol is followed only once its protocol is open; other symbols and private names
    /// are always followed.
    pub(super) fn admitted(&self, closure: &Closure, holder: Value, key: PropertyKey) -> bool {
        match key {
            PropertyKey::String(atom) => {
                closure.wild
                    || closure.names.contains(&atom)
                    || self.coercion_name(closure, holder, atom)
                    || closure.receiver_names.get(&holder).is_some_and(|names| names.contains(&atom))
                    || (closure.indexed && numeric_name(self.atom_name(atom)))
            }
            PropertyKey::Symbol(symbol) => closure.wild || !self.gated_symbol(symbol) || closure.symbols.contains(&symbol),
            PropertyKey::Private(..) => true,
        }
    }

    /// Whether a store's name exposes the setters `holder` holds under it: `setters` admits only accessors.
    pub(super) fn setter_admitted(&self, closure: &Closure, key: PropertyKey) -> bool {
        matches!(key, PropertyKey::String(atom) if closure.setters.contains(&atom))
    }

    /// Whether `valueOf` or `toString` is read on `holder`: the coercion protocol is open, and
    /// `Function.prototype` only when a function may be coerced.
    fn coercion_name(&self, closure: &Closure, holder: Value, atom: Atom) -> bool {
        closure.coercion
            && (atom == self.runtime_atoms.value_of || atom == self.runtime_atoms.to_string)
            && (holder != self.function_proto || closure.function_coercion)
    }

    /// Whether `symbol` is a well-known symbol whose protocol gates its keys.
    fn gated_symbol(&self, symbol: Value) -> bool {
        GATED_SYMBOLS.into_iter().any(|gated| self.well_known_symbols.get(gated) == Some(symbol))
    }

    /// The intrinsic prototypes of the iterators the realm makes, and of the generators: the
    /// receivers `next`, `return` and `throw` are read on. They are reached whatever the program
    /// names, since a native makes iterators the program can step by name.
    pub(super) fn iterator_receivers(&self) -> [Value; 12] {
        [
            self.iterator_proto, self.array_iterator_proto, self.string_iterator_proto, self.map_iterator_proto,
            self.set_iterator_proto, self.regexp_string_iterator_proto, self.generator_proto, self.async_generator_proto,
            self.async_iterator_proto, self.async_from_sync_iterator_proto, self.iterator_helper_proto,
            self.wrap_for_valid_iterator_proto,
        ]
    }

    /// The getter and setter of `holder`'s own accessor `key`; a data property has none.
    pub(super) fn own_accessors(&self, holder: Value, key: PropertyKey) -> Vec<Value> {
        // The slot's attributes and value are record data.
        let Some(data) = self.heap.object(holder).map(|object| object.record()) else { return Vec::new() };
        let Some((_, slot)) = self.object_property_slot(holder, key) else { return Vec::new() };
        match self.slot_attributes(data, slot) {
            Some(attributes) if attributes.accessor => attributes.getter.into_iter().chain(attributes.setter).collect(),
            _ => Vec::new(),
        }
    }

    /// The values of `holder`'s own property `key`: its getter and setter, or its data value.
    pub(super) fn own_values(&self, holder: Value, key: PropertyKey) -> Vec<Value> {
        // The slot's attributes and value are record data.
        let Some(data) = self.heap.object(holder).map(|object| object.record()) else { return Vec::new() };
        let Some((_, slot)) = self.object_property_slot(holder, key) else { return Vec::new() };
        let Some(attributes) = self.slot_attributes(data, slot) else { return Vec::new() };
        let mut values: Vec<Value> = attributes.getter.into_iter().chain(attributes.setter).collect();
        if !attributes.accessor {
            values.extend(self.slot_data_value(data, slot));
        }
        values
    }
}
