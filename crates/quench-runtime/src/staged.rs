//! Operation semantics written once, over a binding-time domain (`Stage`).
//!
//! This is Futamura's first projection made literal through Ershov's mixed computation. Each
//! operation below is one Rust function, generic over the stage that runs it:
//!
//! - The interpreter runs it on machine words (`Concrete`). Every condition is known there, so
//!   only the arm taken runs.
//! - A specializer runs it on IR values, which emits the residual code (`Emit` in
//!   `vm/compiled/object_slots.rs`). A condition it cannot know becomes a guard, or both arms.
//!
//! A plain Rust argument (a slot, a storage form a site proved) is static, so a branch on it
//! leaves no code. A rule written here is the rule of every tier, which is what makes the
//! native tiers the interpreter specialized, not a second implementation of it
//! (`docs/v2/specialization.md`). `staged_coverage` in this module's tests keeps hand-written
//! decodes of the same layouts from reappearing elsewhere.

use crate::bytecode::{Op, Register, WideInstruction};
use crate::typed_ir::Type;

mod concrete;
pub(crate) mod numeric;
mod heap;
mod global;
mod names;
mod callout;
pub(crate) use global::{GlobalBinding, GlobalFrame};
pub(crate) use concrete::Concrete;
pub(crate) use numeric::{Domain, NumberOperation, NumericFrame, NumericStage, Overflow};
pub(crate) mod elements;
pub(crate) mod entry;
pub(crate) mod environment;
pub(crate) mod ic;
pub(crate) mod object;
pub(crate) mod store;
pub(crate) mod strings;
pub(crate) mod holder;
pub(crate) mod call; pub(crate) mod frame;
pub(crate) mod construct;
pub(crate) mod plan_fields;
pub(crate) mod value;
mod steps;
mod generic;
pub(crate) use steps::step;
pub(crate) mod effects;
mod operation_effects;
#[cfg(test)]
pub(crate) mod trace;
#[allow(unused_imports)] // the Stage v2 vocabulary: stencil and UltraJIT stages use the rest
pub(crate) use effects::{CallTarget, Effects, Region, RegionSet};
pub(crate) use ic::{concrete_held_receiver, concrete_own_field_hit, concrete_site_chain, concrete_site_read, concrete_site_store};
pub(crate) use object::{Form, Layout, PackedAccess, Slots, concrete_own_read,
    arena_extras_layout, own_layout, read_slot};

mod stage;
pub(crate) use stage::{Stage, Table};

mod register_frame;
pub(crate) use register_frame::{CallOut, Constant, Frame, Protocol, Step, jump_if, load_this, logical_not, read_local, write_local};

/// An instruction `step` runs, with the operands a tier decoded.
#[derive(Clone, Copy)]
pub(crate) enum Operation<W, P> {
    /// Copy register `source` into `destination` with its facts.
    Move { destination: Register, source: Register },
    /// Define `destination` from `constant`.
    LoadConst { destination: Register, constant: Constant },
    /// Define `destination` from local slot `slot`.
    LoadLocalPlain { destination: Register, slot: u16 },
    /// Write `source` to local slot `slot`, and to `result` where the instruction has one.
    StoreLocalPlain { source: Register, slot: u16, result: Option<Register> },
    /// Continue at the jump target.
    Jump,
    /// Continue at the jump target when register `register` is truthy, or when it is falsy
    /// where `taken` is false; otherwise continue after this instruction.
    JumpIf { register: Register, taken: bool },
    /// Define `destination` with the activation's `this`; where the activation does not
    /// hold it, leave at the frame's miss edge.
    LoadThis { destination: Register },
    /// A Number operation (`numeric`).
    Number(NumberOperation),
    /// Continue where register `register` is neither `null` nor `undefined`; those leave at
    /// the frame's miss edge, where the interpreter throws the TypeError
    /// (RequireObjectCoercible).
    RequireObjectCoercible { register: Register },
    /// Define `destination` with the logical negation of register `source`'s truthiness.
    LogicalNot { destination: Register, source: Register },
    /// The proven slot `slot` of `base`'s object, under `proof`, as a value (`Frame::field_read`).
    ReadField { base: Register, slot: u16, ty: Type, proof: P },
    /// Write `value` to the proven slot `slot` of `base`'s object under `proof`.
    WriteField { base: Register, slot: u16, ty: Type, value: W, proof: P },
    /// A dense array element at `index`, under `proof`; a hole or miss leaves `Unresolved`.
    IndexRead { index: W, proof: P },
    /// Store `value` at dense array element `index`, under `proof`.
    IndexWrite { index: W, value: W, proof: P },
    /// A typed array element at `index`, as a Number word, under `proof`.
    TypedIndexRead { index: W, proof: P },
    /// Store the Number `value` at typed array element `index`, under `proof`.
    TypedIndexWrite { index: W, value: W, proof: P },
    /// The object literal `destination`, allocated by the frame's heap; a due collection
    /// leaves at the frame's miss edge.
    MakeObject { destination: Register, proof: P },
    /// The array literal `destination`: `length` holes, or `elements` values.
    MakeArray { destination: Register, length: W, elements: W, proof: P },
    /// The array literal `destination` of the `count` program constants from `constant`.
    MakeConstArray { destination: Register, constant: W, count: W, proof: P },
    /// The two-property object literal `destination` of literal `site`'s shape, holding
    /// `first` and `second`; a due collection leaves at the frame's miss edge.
    MakeObjectPair { destination: Register, site: u32, first: W, second: W, proof: P },
    /// Continue unless an interrupt or a due collection is requested (`Frame::safepoint`).
    Safepoint { proof: P },
    /// The element store's host helper for `base[key] = value`, under `proof`; the frame's
    /// miss edge takes a failed store.
    IndexSetHelper { base: W, key: W, value: W, proof: P },
    /// The global-name read of `atom` through cache `site`, defined into `destination` as
    /// `binding` says; a miss leaves at the proof's edge.
    GlobalRead { destination: Register, atom: u32, site: u16, binding: GlobalBinding, proof: P },
    /// Define `destination` as `undefined` (`void`).
    Void { destination: Register },
    /// ToPropertyKey of a string `source`, which is its own key; any other value leaves at the
    /// miss edge, where the VM converts it (`Vm::to_property_key`: a Number allocates, an object
    /// runs guest code).
    PropertyKey { destination: Register, source: Register },
    /// `left == right` (or `===`; negated for `!=`, `!==`) where the two words decide it: the
    /// same word, two int32s, or `null` and `undefined` under `==`. Any other pair leaves at the
    /// miss edge, where the VM compares (strings, BigInts, conversions that run guest code).
    Equality { destination: Register, left: Register, right: Register, strict: bool, negated: bool },
    /// Any other `Binary` of two registers (a string or BigInt relation, an arithmetic of other
    /// than Numbers, `instanceof`, `in`): the VM's own operation, a callout.
    Binary { destination: Register, left: Register, right: Register, operator: u32 },
    /// Define `destination` from environment slot `slot` (`environment::environment_slot`); a
    /// sparse storage or an internal marker leaves at the frame's miss edge.
    LoadEnvLocal { destination: Register, slot: u16 },
    /// Write `source` to environment slot `slot`, and to `result` where the instruction has one;
    /// `initialize` where the store is the binding's declaration.
    StoreEnvLocal { source: Register, slot: u16, result: Option<Register>, initialize: bool },
    /// Define `destination` with the length of the arguments binding in local `slot`.
    ArgumentsLength { destination: Register, slot: u16 },
    /// Define `destination` with the `typeof` name of the global `atom` through cache `site`
    /// (the lane's own read); an unresolvable name is `undefined`, not an error.
    GlobalTypeof { destination: Register, atom: u32, site: u16 },
    /// The global-name store of `source` to `atom` through cache `site`, an initialization
    /// where `initialize`; a binding the lane's store does not take leaves at the miss edge.
    GlobalWrite { source: Register, atom: u32, site: u16, initialize: bool },
    /// An iteration or spread step (`Protocol`) over the `operands` registers, defining
    /// `destination` where it has one; the generic protocol runs guest code, a callout.
    Protocol { step: Protocol, destination: Option<Register>, operands: [Option<Register>; 2] },
    /// Define `destination` with the next key of the for-in state in register `state`; continue
    /// at the jump target once none is left.
    ForInNext { destination: Register, state: Register },
    /// The VM field-store helper for `object.atom = value` through cache `site`.
    FieldStoreMiss { object: W, atom: u32, site: u16, value: W, proof: P },
}
impl<W, P> Operation<W, P> {
    /// The staged operation `instruction` denotes, with the operands the catalog fixes.
    /// `None` where the opcode has no staged operation, or where its constant is not
    /// available to this tier: the caller then takes its own exact path. The heap
    /// operations are the frame's (`Frame::field_read`); a tier decodes those itself.
    pub(crate) fn decode(instruction: WideInstruction,
        constant: impl FnOnce(usize) -> Option<Constant>) -> Option<Self> {
        Some(match instruction.op() {
            Op::Move => Self::Move {
                destination: instruction.result_register(), source: instruction.register_b() },
            Op::LoadConst => Self::LoadConst {
                destination: instruction.result_register(),
                constant: constant(instruction.constant_index())? },
            Op::LoadLocalPlain | Op::StoreLocalPlain => return Self::decode_local(instruction),
            Op::Jump => Self::Jump,
            Op::JumpFalse => Self::JumpIf {
                register: instruction.register_a(), taken: false },
            Op::JumpTrue => Self::JumpIf {
                register: instruction.register_a(), taken: true },
            Op::Binary | Op::ToPropertyKey => return Self::decode_generic(instruction),
            Op::LoadThis | Op::RequireObjectCoercible | Op::ToNumeric | Op::Unary => return Self::decode_value(instruction),
            _ => return names::decode(instruction),
        })
    }

    /// The value-level operations of `decode`.
    fn decode_value(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::LoadThis => Self::LoadThis {
                destination: instruction.result_register() },
            Op::RequireObjectCoercible => Self::RequireObjectCoercible {
                register: instruction.register_b() },
            Op::ToNumeric => Self::Number(NumberOperation::ToNumeric {
                destination: instruction.result_register(), source: instruction.register_b() }),
            Op::Unary if instruction.unary_operator()
                == oxc_ast::ast::UnaryOperator::LogicalNot as u32 => Self::LogicalNot {
                    destination: instruction.result_register(), source: instruction.register_b() },
            Op::Unary if instruction.unary_operator() == oxc_ast::ast::UnaryOperator::Void as u32
                && !instruction.returns_from_frame() => Self::Void { destination: instruction.result_register() },
            _ => return None,
        })
    }

    /// The local-slot operations of `decode`.
    fn decode_local(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::LoadLocalPlain => Self::LoadLocalPlain {
                destination: instruction.result_register(),
                slot: u16::try_from(instruction.local_slot()).ok()? },
            Op::StoreLocalPlain => Self::StoreLocalPlain {
                source: instruction.register_a(),
                slot: u16::try_from(instruction.local_slot()).ok()?,
                result: instruction.optional_register_b() },
            _ => return None,
        })
    }
}

/// Which of an instruction's two control edges it takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Edge {
    /// The jump target.
    Target,
    /// The instruction after this one.
    Following,
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod coverage_tests;
#[cfg(test)]
mod generic_tests;
