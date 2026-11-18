//! The VM operations a staged row calls out for when its own words do not decide it, named as
//! data. A frame answers one in place (`Frame::vm_step`): the interpreter's frame calls the VM
//! method, the lane specializer's frame calls the lane helper that decodes the record and makes
//! the same step (`dispatch_fast::lane_vm_step`), and a frame with no host declines.

use super::spread::Passing;
use super::{Arguments, NumericFrame, NumericStage, Operation, Protocol, Register, Step};

/// The tag byte of each `CallOut`, which a helper-call row passes as its immediate.
pub(crate) const TAG_EQUALITY: u8 = 0;
pub(crate) const TAG_BINARY: u8 = 1;
pub(crate) const TAG_PROPERTY_KEY: u8 = 2;
pub(crate) const TAG_PROTOCOL: u8 = 3;
pub(crate) const TAG_FOR_IN_NEXT: u8 = 4;
pub(crate) const TAG_LOAD_ENV_LOCAL: u8 = 5;
pub(crate) const TAG_STORE_ENV_LOCAL: u8 = 6;
pub(crate) const TAG_ARGUMENTS_LENGTH: u8 = 7;
pub(crate) const TAG_GLOBAL_WRITE: u8 = 8;
pub(crate) const TAG_GLOBAL_TYPEOF: u8 = 9;
pub(crate) const TAG_SPREAD: u8 = 10;
pub(crate) const TAG_DELETE: u8 = 11;
pub(crate) const TAG_LOAD_NAME: u8 = 12;
pub(crate) const TAG_SUPER_CALL: u8 = 13;
pub(crate) const TAG_DECIDE: u8 = 14;

/// A VM operation and the registers and immediates it reads: plain data, whose tag byte
/// (`repr(u8)`) and fields a helper-call row passes as immediates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(crate) enum CallOut {
    /// `left == right`, or `===` where `strict`, negated for `!=` and `!==`, by the VM's full
    /// equality: strings and BigInts compare by contents, a conversion may run guest code.
    Equality { left: Register, right: Register, strict: bool, negated: bool } = TAG_EQUALITY,
    /// `left operator right` by the VM's `Vm::binary`: a string or BigInt relation or operator,
    /// `instanceof`, `in`.
    Binary { left: Register, right: Register, operator: u32 } = TAG_BINARY,
    /// ToPropertyKey of `source` (`Vm::to_property_key`): a Number allocates, an object runs
    /// guest code.
    PropertyKey { source: Register } = TAG_PROPERTY_KEY,
    /// An iteration or spread step over its operand registers (`Protocol`).
    Protocol { step: Protocol, operands: [Option<Register>; 2] } = TAG_PROTOCOL,
    /// The next key of the for-in state in `state`; `Value::DELETED`, which no key is, once
    /// none is left.
    ForInNext { state: Register } = TAG_FOR_IN_NEXT,
    /// The environment local `slot` of the running frame, by the binding rules.
    LoadEnvLocal { slot: u16 } = TAG_LOAD_ENV_LOCAL,
    /// `source` to the environment local `slot` of the running frame, by the binding rules (a
    /// `with` scope, TDZ, a constant), `initialize` where it declares the binding.
    StoreEnvLocal { source: Register, slot: u16, initialize: bool } = TAG_STORE_ENV_LOCAL,
    /// The length of the arguments binding in local `slot`.
    ArgumentsLength { slot: u16 } = TAG_ARGUMENTS_LENGTH,
    /// `source` to the global name `atom` through cache `site`, `initialize` for a declaration.
    GlobalWrite { source: Register, atom: u32, site: u16, initialize: bool } = TAG_GLOBAL_WRITE,
    /// The `typeof` name of the global `atom` through cache `site`.
    GlobalTypeof { atom: u32, site: u16 } = TAG_GLOBAL_TYPEOF,
    /// `delete base[key]` by the VM's `delete_reference_property_mode`, which throws on a
    /// refusal where `strict` or the running function is strict.
    Delete { base: Register, key: Register, strict: bool } = TAG_DELETE,
    /// The value of the name `atom` through cache `site`, by the binding rules of the running
    /// frame (`Vm::load_name`: a `with` scope, a global, an uninitialized binding).
    LoadName { atom: u32, site: u16 } = TAG_LOAD_NAME,
    /// `callee` called with `receiver` and the arguments a spread makes of the registers in
    /// `arguments` (`Passing`), by the VM's call: the argument iteration runs guest code.
    Spread { callee: Register, receiver: Register, arguments: Arguments, passing: Passing } = TAG_SPREAD,
    /// `callee` constructed by `super(...)` with the arguments in `arguments`, by the VM's
    /// construction with the running activation's `new.target`.
    SuperCall { callee: Register, arguments: Arguments, passing: Passing } = TAG_SUPER_CALL,
    /// `left operator right` decided by the VM (`Vm::binary_truthy`): a comparison jump's slow
    /// case. It defines no register; its answer is a canonical boolean, which a lane's helper
    /// reports as its status (`LANE_CALL_HOLDS`).
    Decide { left: Register, right: Register, operator: u32 } = TAG_DECIDE,
}

/// The registers the VM reads for a `CallOut`: up to two of fixed place, and a list a call
/// passes (`Arguments`), which a handle names without copying it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Operands {
    pub(crate) fixed: [Option<Register>; 2],
    pub(crate) list: Option<Arguments>,
}

/// The values of a `CallOut`'s operands, read by the tier from its own registers.
pub(crate) struct OperandValues<V> {
    pub(crate) fixed: [Option<V>; 2],
    pub(crate) list: Vec<V>,
}

impl Operands {
    /// Operands of fixed place alone.
    const fn of_fixed(fixed: [Option<Register>; 2]) -> Self {
        Self { fixed, list: None }
    }

    /// Every register the operands name, with the list's resolved by `records` (the call records
    /// of a method site; a window needs none).
    pub(crate) fn registers<'a>(self, records: &'a [Register]) -> impl Iterator<Item = Register> + 'a {
        self.fixed.into_iter().flatten().chain(self.list.into_iter().flat_map(move |list| list.registers(records)))
    }

    /// The operands' values, each register read by `read` in the order `registers` names them.
    pub(crate) fn read<V>(self, records: &[Register], mut read: impl FnMut(Register) -> V) -> OperandValues<V> {
        let fixed = self.fixed.map(|register| register.map(&mut read));
        let list = self.list.map_or_else(Vec::new, |list| list.registers(records).map(&mut read).collect());
        OperandValues { fixed, list }
    }
}

impl CallOut {
    /// The operands and operator of a step the VM decides from the two words alone, with no
    /// effect: `===` and `!==` (`Vm::strict_equal`), as a decision or a value. It runs no guest
    /// code, allocates nothing and never raises, so a tier may ask a pure helper of the words
    /// (`dispatch_fast::lane_decide`) instead of making the step in place.
    pub(crate) fn word_decision(self) -> Option<([Register; 2], u32)> {
        use crate::vm::operations::{equality_binary_operator, equality_operator};
        match self {
            Self::Decide { left, right, operator } if equality_operator(operator).is_some_and(|(strict, _)| strict) =>
                Some(([left, right], operator)),
            Self::Equality { left, right, strict: true, negated } => Some(([left, right], equality_binary_operator(true, negated))),
            _ => None,
        }
    }

    /// Whether the step reads or writes the locals of the running activation, or resolves a name
    /// through its bindings (`Vm::load_name` searches its scope): its owner is the activation's
    /// (`Vm::running_owner`), not the innermost frame's, which beside the lane's frameless
    /// activations is their caller's.
    pub(crate) fn names_activation(self) -> bool {
        matches!(self, Self::LoadEnvLocal { .. } | Self::StoreEnvLocal { .. } | Self::LoadName { .. }
            | Self::GlobalTypeof { .. } | Self::GlobalWrite { .. })
    }

    /// The registers the VM reads, in the order its operation takes them: the first operand
    /// then the second, which is the only one an operation of a single operand has, then the
    /// list of arguments a call passes.
    pub(crate) fn operands(self) -> Operands {
        Operands::of_fixed(match self {
            Self::Equality { left, right, .. } | Self::Binary { left, right, .. } | Self::Delete { base: left, key: right, .. }
                | Self::Decide { left, right, .. } => [Some(left), Some(right)],
            Self::PropertyKey { source: operand } | Self::ForInNext { state: operand } => [None, Some(operand)],
            Self::StoreEnvLocal { source: operand, .. } | Self::GlobalWrite { source: operand, .. } => [Some(operand), None],
            Self::LoadEnvLocal { .. } | Self::ArgumentsLength { .. } | Self::GlobalTypeof { .. } | Self::LoadName { .. } => [None, None],
            Self::Protocol { operands, .. } => operands,
            Self::Spread { callee, receiver, arguments, .. } =>
                return Operands { fixed: [Some(callee), Some(receiver)], list: Some(arguments) },
            Self::SuperCall { callee, arguments, .. } => return Operands { fixed: [Some(callee), None], list: Some(arguments) },
        })
    }
}

impl<W, P> Operation<W, P> {
    /// The VM operation of a row that calls out, and the register it defines, if any; `None`
    /// for a row that never does.
    pub(crate) fn call_out(&self) -> Option<(CallOut, Option<Register>)> {
        Some(match *self {
            Self::PropertyKey { destination, source } => (CallOut::PropertyKey { source }, Some(destination)),
            Self::Equality { destination, left, right, strict, negated } =>
                (CallOut::Equality { left, right, strict, negated }, Some(destination)),
            Self::Binary { destination, left, right, operator } =>
                (CallOut::Binary { left, right, operator }, Some(destination)),
            Self::Protocol { step, destination, operands } => (CallOut::Protocol { step, operands }, destination),
            Self::ForInNext { destination, state } => (CallOut::ForInNext { state }, Some(destination)),
            _ => return self.binding_call_out(),
        })
    }

    /// The environment and global rows of `call_out`.
    fn binding_call_out(&self) -> Option<(CallOut, Option<Register>)> {
        Some(match *self {
            Self::LoadEnvLocal { destination, slot } => (CallOut::LoadEnvLocal { slot }, Some(destination)),
            Self::StoreEnvLocal { source, slot, result, initialize } =>
                (CallOut::StoreEnvLocal { source, slot, initialize }, result),
            Self::ArgumentsLength { destination, slot } => (CallOut::ArgumentsLength { slot }, Some(destination)),
            Self::GlobalWrite { source, atom, site, initialize } =>
                (CallOut::GlobalWrite { source, atom, site, initialize }, None),
            Self::GlobalTypeof { destination, atom, site } => (CallOut::GlobalTypeof { atom, site }, Some(destination)),
            Self::Delete { destination, base, key, strict } => (CallOut::Delete { base, key, strict }, Some(destination)),
            Self::CompareJump { comparison, left, right, .. } => (CallOut::Decide { left: left.register_index()?,
                right: right.register_index()?, operator: comparison.operator()? }, None),
            _ => return self.call_out_of_call(),
        })
    }

    /// The VM operation of a call row the VM gathers the arguments of (`spread`), and the register it
    /// defines; `None` for any other row.
    fn call_out_of_call(&self) -> Option<(CallOut, Option<Register>)> {
        Some(match *self {
            Self::SpreadCall { destination, callee, receiver, arguments, passing } =>
                (CallOut::Spread { callee, receiver, arguments, passing }, Some(destination)),
            Self::SuperCall { destination, callee, arguments, passing } =>
                (CallOut::SuperCall { callee, arguments, passing }, Some(destination)),
            _ => return None,
        })
    }
}

/// A row whose operation is the VM's: the frame makes it, and the word it defines, if any, is
/// `destination`'s; a frame with no host leaves it to the interpreter.
#[inline(always)]
pub(super) fn calling<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, call: CallOut,
    destination: Option<Register>) -> Step<S::Word, F::Control> {
    match frame.vm_step(stage, call, destination) {
        Some((value, fact)) => {
            if let Some(destination) = destination { frame.write(stage, destination, value, fact); }
            Step::Value(value)
        }
        None => Step::Unresolved,
    }
}
