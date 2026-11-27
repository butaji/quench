//! The protocol gates of the closed world: the string names and well-known symbols a runtime
//! protocol reads on a value the program did not name. An opcode or a native opens the gate of the
//! protocol it runs; a closed gate keeps its keys out of the closure. Opening a gate revisits what
//! the closure has already reached, since a reached object may expose the key the gate now admits.

use super::*;
use super::reachable_realm::Closure;
use crate::bytecode::{Op, WideInstruction};
use crate::operator::{BinaryOperator, UnaryOperator};

/// The names an opcode's protocol reads implicitly: the thrown value's report and a thenable check.
pub(super) fn protocol_names(op: Op) -> &'static [&'static str] {
    match op {
        Op::Throw => &["message", "stack", "name"],
        Op::Await | Op::Yield | Op::YieldStar | Op::GetAsyncIterator => &["then"],
        _ => &[],
    }
}

/// Opcodes whose property key is computed at run time, so any key of a reached object may be read.
pub(super) fn computed_key_op(op: Op) -> bool {
    matches!(
        op,
        Op::GetIndex | Op::SetIndex | Op::ToPropertyKey | Op::DefineComputedField | Op::ForInStart | Op::ForInNext
            | Op::GetEnumerated | Op::CopyDataProperties | Op::Delete
    )
}

/// The well-known symbols whose keys are followed only once their protocol is open.
const GATED_SYMBOLS: [WellKnownSymbol; 4] = [
    WellKnownSymbol::ToPrimitive, WellKnownSymbol::Iterator, WellKnownSymbol::AsyncIterator, WellKnownSymbol::HasInstance,
];

/// The protocols a native runs on its arguments or its `this`, by its family.
pub(super) struct NativeProtocols {
    /// Runs the iteration protocol over an argument (`next`, `return`, `throw`).
    pub(super) iterates: bool,
    /// Constructs its `this` or species, so reads the constructor's `prototype`.
    pub(super) constructs: bool,
    /// Resolves or chains a promise, so reads a value's `then`.
    pub(super) promises: bool,
}

pub(super) fn native_protocols(native: Native) -> NativeProtocols {
    let name = format!("{native:?}");
    NativeProtocols {
        iterates: starts_with_any(&name, &["Iterator", "AsyncFromSync", "PromiseAll", "PromiseRace", "PromiseAny", "ArrayFrom", "TypedArrayFrom"])
            || matches!(
                native,
                Native::Map | Native::Set | Native::WeakMap | Native::WeakSet | Native::MapGroupBy | Native::ObjectFromEntries
                    | Native::ObjectGroupBy | Native::SetIsDisjointFrom | Native::SetIsSubsetOf | Native::SetIsSupersetOf
            ),
        constructs: starts_with_any(&name, &["Array", "TypedArray", "Promise", "Reflect", "ArrayBuffer", "SharedArrayBuffer", "RegExp"]),
        promises: starts_with_any(&name, &["Promise", "Async"]),
    }
}

fn starts_with_any(name: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| name.starts_with(prefix))
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

impl Vm {
    /// Opens the protocols one instruction runs on its operands.
    pub(super) fn open_instruction(&mut self, closure: &mut Closure, instruction: WideInstruction) {
        match instruction.op() {
            Op::Binary => self.open_binary(closure, instruction.binary_operator()),
            Op::Unary => {
                let operator = instruction.unary_operator();
                if [UnaryOperator::UnaryPlus, UnaryOperator::UnaryNegation, UnaryOperator::BitwiseNot]
                    .iter()
                    .any(|coerces| *coerces as u32 == operator)
                {
                    self.open_coercion(closure);
                }
            }
            op => self.open_op(closure, op),
        }
    }

    /// Opens the protocols of a non-operator opcode: iteration, construction and key coercion.
    fn open_op(&mut self, closure: &mut Closure, op: Op) {
        match op {
            Op::GetIterator | Op::GetAsyncIterator | Op::IteratorClose | Op::IteratorCleanupPush | Op::IteratorCleanupPop
            | Op::YieldStar | Op::AppendArray | Op::CallSpread => self.open_iteration(closure),
            Op::Construct | Op::ValidateClassHeritage => open_name(closure, self.prototype_atom),
            Op::NumericAdd | Op::NumericMultiply | Op::ToNumeric | Op::IncDec | Op::StringConcat | Op::ToPropertyKey
            | Op::SetFunctionNameKey | Op::Delete | Op::GetIndex | Op::SetIndex | Op::DefineComputedField => {
                self.open_coercion(closure)
            }
            _ => {}
        }
    }

    /// `instanceof` reads the right side's `@@hasInstance` and `prototype`. Every binary operator
    /// but the strict comparisons and `instanceof` coerces an operand.
    fn open_binary(&mut self, closure: &mut Closure, operator: u32) {
        if operator == BinaryOperator::Instanceof as u32 {
            open_name(closure, self.prototype_atom);
            self.open_well_known(closure, WellKnownSymbol::HasInstance);
        } else if operator != BinaryOperator::StrictEquality as u32 && operator != BinaryOperator::StrictInequality as u32 {
            self.open_coercion(closure);
        }
    }

    /// Opens the coercion protocol: `valueOf`, `toString` and `@@toPrimitive` are read on an object
    /// a coercion reaches. A native may coerce any argument it takes, so the closure opens this
    /// protocol once it reaches one.
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

    fn open_well_known(&self, closure: &mut Closure, symbol: WellKnownSymbol) {
        if let Some(value) = self.well_known_symbols.get(symbol) {
            if closure.symbols.insert(value) {
                requeue(closure);
            }
        }
    }

    /// Whether `symbol` is a well-known symbol whose protocol gates its keys.
    pub(super) fn gated_symbol(&self, symbol: Value) -> bool {
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
}
