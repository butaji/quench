//! The VM operations a staged row calls out for when its own words do not decide it, named as
//! data. A frame answers one in place (`Frame::vm_step`): the interpreter's frame calls the VM
//! method, the lane specializer's frame calls the lane helper that decodes the record and makes
//! the same step (`dispatch_fast::vm_operation_step`), and a frame with no host declines.

use super::spread::Passing;
use super::{Arguments, NumericFrame, NumericStage, Operation, Protocol, Register, Step};
use crate::bytecode::{FieldLookup, Operand};

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
pub(crate) const TAG_ENUMERATED_GET: u8 = 15;
pub(crate) const TAG_CLONE_ENV: u8 = 16;
pub(crate) const TAG_ARGUMENT_GET: u8 = 17;
// merge: HEAD's Accessor keeps its variant (used by vm/call_out.rs) under a fresh tag, since theirs took 16 and 17.
pub(crate) const TAG_ACCESSOR: u8 = 18;
pub(crate) const TAG_TO_NUMERIC: u8 = 19;
pub(crate) const TAG_INC_DEC: u8 = 20;
pub(crate) const TAG_UNARY: u8 = 21;
pub(crate) const TAG_STORE_FIELD: u8 = 22;
pub(crate) const TAG_STORE_ELEMENT: u8 = 23;
pub(crate) const TAG_LOAD_FIELD: u8 = 24;
pub(crate) const TAG_LOAD_ELEMENT: u8 = 25;
pub(crate) const TAG_DECIDE_OPERAND: u8 = 26;
pub(crate) const TAG_GLOBAL_LEXICAL_LOAD: u8 = 27;
pub(crate) const TAG_GLOBAL_LEXICAL_STORE: u8 = 28;

/// A VM operation and the registers and immediates it reads: plain data, whose tag byte
/// (`repr(u8)`) and fields a helper-call row passes as immediates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum CallOut {
    /// `left == right`, or `===` where `strict`, negated for `!=` and `!==`, by the VM's full
    /// equality: strings and BigInts compare by contents, a conversion may run guest code.
    Equality { left: Operand, right: Operand, strict: bool, negated: bool } = TAG_EQUALITY,
    /// `left operator right` by the VM's `Vm::binary`: a string or BigInt relation or operator,
    /// `instanceof`, `in`. An operand may be a program constant (`Operand::constant`).
    Binary { left: Operand, right: Operand, operator: u32 } = TAG_BINARY,
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
    /// The element `index` of the arguments binding in local `slot` (the VM's read, by the binding rules).
    ArgumentGet { index: Register, slot: u16 } = TAG_ARGUMENT_GET,
    /// `source` to the global name `atom` through cache `site`, `initialize` for a declaration.
    GlobalWrite { source: Register, atom: u32, site: u16, initialize: bool } = TAG_GLOBAL_WRITE,
    /// The `typeof` name of the global `atom` through cache `site`.
    GlobalTypeof { atom: u32, site: u16 } = TAG_GLOBAL_TYPEOF,
    /// The script-top-level binding `declaration` names, read through cache `site` (`LoadGlobalLexical`).
    GlobalLexicalLoad { declaration: u32, site: u16 } = TAG_GLOBAL_LEXICAL_LOAD,
    /// `source` to the script-top-level binding `declaration` names, through cache `site`, `initialize`
    /// for a declaration (`StoreGlobalLexical`).
    GlobalLexicalStore { source: Register, declaration: u32, site: u16, initialize: bool } = TAG_GLOBAL_LEXICAL_STORE,
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
    /// `left operator right` decided by the VM as `Decide` is, where either operand may be a program constant
    /// (`Operand::constant`): the truth of the general `Binary` result (`ToBoolean`, `Vm::binary`). Used by a
    /// comparison jump whose operand is a constant the words do not decide.
    DecideOperand { left: Operand, right: Operand, operator: u32 } = TAG_DECIDE_OPERAND,
    /// `base[key]` of a for-in's enumerated read whose state's memo did not answer: the element read.
    EnumeratedGet { base: Register, key: Register } = TAG_ENUMERATED_GET,
    /// The running activation's environment cloned by environment clone plan `plan`, the copy a
    /// `for (let ...)` iteration's closures capture. It defines no register.
    CloneEnv { plan: u32 } = TAG_CLONE_ENV,
    /// The `[[Get]]` of `atom` on `receiver` through cache `site` by the VM's field read
    /// (`Vm::get_field_cached`): where the site cached a getter, the getter runs by the
    /// interpreter's own activation (`call_value`) with `receiver` as `this`. The getter is the
    /// site's cached accessor, which no register holds, so the VM resolves it from the site.
    #[allow(dead_code)]
    Accessor { receiver: Register, atom: u32, site: u16 } = TAG_ACCESSOR,
    /// ToNumeric of `source` (`Vm::to_numeric`): a non-Number value's conversion, which may run guest code.
    ToNumeric { source: Register } = TAG_TO_NUMERIC,
    /// The `++` (or `--`) of `source` (`decrement`) by the VM's update: a Number's step, else ToNumeric and
    /// the BigInt update (`Vm::inc_dec_value`). Defines the updated value.
    IncDec { source: Register, decrement: bool } = TAG_INC_DEC,
    /// The unary operator `operator` (a `UnaryOperator`) of `value` by the VM's `unary`: a non-int32
    /// operand's negation, plus or bitwise not. Defines the result.
    Unary { operator: u32, value: Register } = TAG_UNARY,
    /// `object.atom = value` through cache `site` by the VM's named store (`Vm::set_field_cached`), which
    /// refuses by throwing where `strict`. `object` is the register the store names, or `None` for the frame's
    /// `this` (a store to `this`, which the VM reads with `checked_this_binding`). Defines no register.
    StoreField { object: Option<Register>, value: Register, atom: u32, site: u16, strict: bool } = TAG_STORE_FIELD,
    /// `object[index] = value` by the VM's keyed store (`Vm::set_index_mode`), strict where `strict` or the
    /// running function is. The value is the operand list's one register (`Arguments::Window`). Defines no register.
    StoreElement { object: Register, index: Register, value: Register, strict: bool } = TAG_STORE_ELEMENT,
    /// The named read `lookup` (`Vm::load_field_step`): a site's nested chain, or an atom read of its base
    /// through the cache, a getter running in place where the frame takes it. Defines the read's value.
    LoadField { lookup: FieldLookup } = TAG_LOAD_FIELD,
    /// `base[key]` by the VM's indexed read (`Vm::load_element_step`), a symbol key's getter in place as
    /// `LoadField`'s. Defines the read's value.
    LoadElement { base: Operand, key: Operand } = TAG_LOAD_ELEMENT,
}

/// The registers the VM reads for a `CallOut`: up to two of fixed place, and a list a call
/// passes (`Arguments`), which a handle names without copying it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Operands {
    /// The fixed operands: a register or a program constant (`Operand`).
    pub(crate) fixed: [Option<Operand>; 2],
    pub(crate) list: Option<Arguments>,
}

/// The values of a `CallOut`'s operands, read by the tier from its own registers or constants.
pub struct OperandValues<V> {
    pub(crate) fixed: [Option<V>; 2],
    #[cfg(not(stencil_build))]
    pub(crate) list: Vec<V>,
}

impl Operands {
    /// Operands of register place alone.
    fn of_fixed(fixed: [Option<Register>; 2]) -> Self {
        Self::of_operands(fixed.map(|register| register.map(Operand::register)))
    }

    /// Operands of fixed place alone, each a register or a constant.
    const fn of_operands(fixed: [Option<Operand>; 2]) -> Self {
        Self { fixed, list: None }
    }

    /// Every register the operands name, with the list's resolved by `records` (the call records
    /// of a method site; a window needs none). A constant operand names no register.
    pub(crate) fn registers<'a>(self, records: &'a [Register]) -> impl Iterator<Item = Register> + 'a {
        self.fixed.into_iter().flatten().filter_map(Operand::register_index)
            .chain(self.list.into_iter().flat_map(move |list| list.registers(records)))
    }

    /// The operands' values, each operand read by `read` in the order `registers` names them, the
    /// list's as registers.
    #[cfg(not(stencil_build))]
    pub(crate) fn read<V>(self, records: &[Register], mut read: impl FnMut(Operand) -> V) -> OperandValues<V> {
        let fixed = self.fixed.map(|operand| operand.map(&mut read));
        let list = self.list.map_or_else(Vec::new, |list| list.registers(records).map(Operand::register).map(&mut read).collect());
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
            Self::Equality { left, right, strict: true, negated } =>
                Some(([left.register_index()?, right.register_index()?], equality_binary_operator(true, negated))),
            _ => None,
        }
    }

    /// Whether the step reads or writes the locals of the running activation, or resolves a name
    /// through its bindings (`Vm::load_name` searches its scope): its owner is the activation's
    /// (`Vm::running_owner`), not the innermost frame's, which beside the lane's frameless
    /// activations is their caller's.
    pub(crate) fn names_activation(self) -> bool {
        matches!(self, Self::LoadEnvLocal { .. } | Self::StoreEnvLocal { .. } | Self::LoadName { .. }
            | Self::GlobalTypeof { .. } | Self::GlobalWrite { .. } | Self::GlobalLexicalLoad { .. } | Self::GlobalLexicalStore { .. })
    }

    /// The registers the VM reads, in the order its operation takes them: the first operand
    /// then the second, which is the only one an operation of a single operand has, then the
    /// list of arguments a call passes.
    pub(crate) fn operands(self) -> Operands {
        let fixed = match self {
            Self::Equality { left, right, .. } | Self::Binary { left, right, .. } | Self::DecideOperand { left, right, .. }
                | Self::LoadElement { base: left, key: right } => return Operands::of_operands([Some(left), Some(right)]),
            Self::Delete { base: left, key: right, .. }
                | Self::Decide { left, right, .. } | Self::EnumeratedGet { base: left, key: right } => [Some(left), Some(right)],
            Self::PropertyKey { source: operand } | Self::ForInNext { state: operand }
                | Self::ToNumeric { source: operand } | Self::IncDec { source: operand, .. }
                | Self::Unary { value: operand, .. } => [None, Some(operand)],
            Self::StoreField { object, value, .. } => return Operands::of_fixed([object, Some(value)]),
            Self::StoreElement { object, index, value, .. } => return Operands {
                fixed: [Some(Operand::register(object)), Some(Operand::register(index))],
                list: Some(Arguments::Window { first: value, count: 1 }),
            },
            Self::ArgumentGet { index, .. } => [Some(index), None],
            Self::StoreEnvLocal { source: operand, .. } | Self::GlobalWrite { source: operand, .. }
                | Self::GlobalLexicalStore { source: operand, .. } => [Some(operand), None],
            Self::LoadEnvLocal { .. } | Self::ArgumentsLength { .. } | Self::GlobalTypeof { .. } | Self::LoadName { .. }
                | Self::CloneEnv { .. } | Self::LoadField { .. } | Self::GlobalLexicalLoad { .. } => [None, None],
            Self::Accessor { receiver, .. } => [Some(receiver), None],
            Self::Protocol { operands, .. } => operands,
            Self::Spread { callee, receiver, arguments, .. } =>
                return Operands { fixed: [Some(Operand::register(callee)), Some(Operand::register(receiver))], list: Some(arguments) },
            Self::SuperCall { callee, arguments, .. } =>
                return Operands { fixed: [Some(Operand::register(callee)), None], list: Some(arguments) },
        };
        Operands::of_fixed(fixed)
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
            Self::EnumeratedGet { destination, base, key, .. } => (CallOut::EnumeratedGet { base, key }, Some(destination)),
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
            Self::ArgumentGet { destination, index, slot } => (CallOut::ArgumentGet { index, slot }, Some(destination)),
            Self::CloneEnv { plan } => (CallOut::CloneEnv { plan }, None),
            Self::GlobalWrite { source, atom, site, initialize } =>
                (CallOut::GlobalWrite { source, atom, site, initialize }, None),
            Self::GlobalTypeof { destination, atom, site } => (CallOut::GlobalTypeof { atom, site }, Some(destination)),
            Self::GlobalLexicalLoad { destination, declaration, site } =>
                (CallOut::GlobalLexicalLoad { declaration, site }, Some(destination)),
            Self::GlobalLexicalStore { source, declaration, site, initialize } =>
                (CallOut::GlobalLexicalStore { source, declaration, site, initialize }, None),
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

/// A store row's VM call-out where the frame takes the general operations in place
/// (`NumericFrame::general_stores`): the word the store answers, else `None`, where the row leaves.
#[inline(always)]
pub(super) fn storing<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, call: CallOut) -> Option<S::Word> {
    if !frame.general_stores() { return None; }
    frame.vm_step(stage, call, None).map(|(word, _)| word)
}

/// A load row's VM call-out, defining `destination`, where the frame takes the general reads in place
/// (`NumericFrame::general_loads`); otherwise the row leaves.
#[inline(always)]
pub(super) fn loading<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, call: CallOut,
    destination: Register) -> Step<S::Word, F::Control> {
    if !frame.general_loads() { return Step::Unresolved; }
    calling(stage, frame, call, Some(destination))
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
