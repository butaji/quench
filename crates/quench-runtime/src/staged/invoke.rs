//! A call and a return as staged operations, written once for every projection. `invoke` is the
//! call protocol (`protocol`) in order: the callee's identity, its `this` (or a construction's
//! receiver), the room both stacks keep, its window, its entry, its completion. The frame holds
//! what its binding time fixed and answers through `CallFrame`:
//!
//! - which entry reaches the callee (`CallFrame::call_entry`): a direct entry of a published body,
//!   whose callee kind the frame folds under the dependency that retires the caller when it changes
//!   and whose identity is checked once per call (`Identity::Guarded`) or folded the same way
//!   (`Identity::Folded`: a known function, a Stable callee), or the VM's own call
//!   (`CallFrame::call_helper`) for any callee a site is not bound to: a native, a bound function,
//!   a callee whose identity changed;
//! - what the call's form reads (`Invocation`): the callee's register, a function the record names,
//!   or a method its receiver's property rows load; a receiver register, `this` or none; a window
//!   of arguments or a list of registers;
//! - where the words live: the caller's registers, the stacks, the entry itself.
//!
//! The interpreter's call admits through the same rows on machine words (`protocol`'s
//! `concrete_*`); the u-jit's lane frame emits them. `ret` completes an activation with a register.

use super::property::get_field;
use super::protocol::{bind_this, complete_call, fill_call, guard_callee, guard_received};
use super::{word_bytes, word_offset, Arguments, CallFacts, CallOperation, Callee, Frame, Invocation, Operation, Passed, PropertyFrame,
    Receiver, Register, RegisterList, Stage, Step};
use crate::bytecode::{FieldBase, FieldLookup};
use crate::Value;

/// The kinds of callee a site binds to a direct entry. A native has no body to enter and a bound
/// function forwards to another callee, so both take the VM's call (`CallEntry::Helper`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CalleeKind {
    /// A script function called with a receiver.
    Script,
    /// A forwarding class constructor: the construction runs its initializer with the receiver
    /// the constructor's plan allocates, and yields that receiver.
    Class,
}

impl CalleeKind {
    /// Whether the call allocates the callee's receiver (a construction) instead of binding the
    /// `this` of a receiver register.
    pub(crate) const fn allocates_receiver(self) -> bool {
        matches!(self, Self::Class)
    }
}

/// How a direct call knows its callee is the one its plan was made for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Identity {
    /// The identity varies per execution: the call checks it once (`guard_callee`).
    Guarded(u64),
    /// The identity is a constant of the call: a function the record names (Static), or a callee
    /// the frame proved Stable and folded under the dependency that retires the caller when it
    /// changes. The call reads and checks nothing.
    Folded(u64),
}

impl Identity {
    /// The callee's identity word.
    pub(crate) const fn word(self) -> u64 {
        match self {
            Self::Guarded(word) | Self::Folded(word) => word,
        }
    }
}

/// What a frame knows of a site's callee at the time it is compiled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct DirectPlan {
    pub(crate) kind: CalleeKind,
    /// The callee's identity, which the site was bound to.
    pub(crate) identity: Identity,
    /// The global object a sloppy callee binds for `undefined` and `null`; `None` for a strict one.
    pub(crate) sloppy: Option<u64>,
    /// The registers of the callee's window.
    pub(crate) registers: u16,
    /// The status word of a callee that returned.
    pub(crate) returned: u32,
}

/// How a call reaches its callee.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CallEntry {
    Direct(DirectPlan),
    Helper,
}

/// The stacks a direct call is admitted to: the callee's entry cell, its window at `top` (its
/// end at `next`), and the frame below at `shadow`.
#[derive(Clone, Copy)]
pub(crate) struct Room<W> {
    pub(crate) cell: W,
    pub(crate) top: W,
    pub(crate) next: W,
    pub(crate) shadow: W,
}

/// A callee's entry: its `status` and result `word`, and where a status other than returned goes.
#[derive(Clone, Copy)]
pub(crate) struct Entered<W, M> {
    pub(crate) status: W,
    pub(crate) word: W,
    pub(crate) unfinished: M,
}

/// The call hooks of a frame. Each defaults to a frame with no direct entry, whose every call is
/// the VM's own.
pub(crate) trait CallFrame<S: Stage>: Frame<S> {
    /// How the call of `site` reaches its callee, by what the frame fixed.
    #[inline(always)]
    fn call_entry(&self, _site: &Invocation) -> CallEntry {
        CallEntry::Helper
    }

    /// The base of the caller's register file, where a call's callee, receiver and arguments are.
    #[inline(always)]
    fn call_registers(&mut self, _stage: &mut S) -> Option<S::Word> {
        None
    }

    /// The registers of a method call's argument list, from the call records the frame decoded the
    /// call with; `None` where the frame holds no records.
    #[inline(always)]
    fn call_list(&self, _list: RegisterList) -> Option<&[Register]> {
        None
    }

    /// The method `atom` (method site `site`) of `receiver`, loaded through the frame's property
    /// rows; `None` where the frame has no plan for it, leaving at `miss` a method that is not there.
    #[inline(always)]
    fn call_method(&mut self, _stage: &mut S, _receiver: S::Word, _atom: u32, _site: u32, _miss: S::Miss) -> Option<S::Word> {
        None
    }

    /// The receiver `constructor`'s plan allocates; the deleted marker where it declines.
    #[inline(always)]
    fn call_receiver(&mut self, _stage: &mut S, _constructor: S::Word) -> Option<S::Word> {
        None
    }

    /// Admit the direct call to the stacks: leaving at `miss` unless both hold the callee's window
    /// and what its body needs to recover.
    #[inline(always)]
    fn call_room(&mut self, _stage: &mut S, _miss: S::Miss) -> Option<Room<S::Word>> {
        None
    }

    /// Push the callee's frame, which holds `this`, and call the entry of its cell with its
    /// window filled.
    #[inline(always)]
    fn call_enter(&mut self, _stage: &mut S, _this: S::Word, _room: Room<S::Word>) -> Option<Entered<S::Word, S::Miss>> {
        None
    }

    /// The call returned `value`: pop its frame and define the call's result, where a status
    /// other than returned goes the way the frame routes it.
    #[inline(always)]
    fn call_settle(&mut self, _stage: &mut S, _site: &Invocation, _entered: Entered<S::Word, S::Miss>, _room: Room<S::Word>,
        _value: S::Word) -> Option<()> {
        None
    }

    /// The VM's own call of `site`, which a call that took no direct entry runs; `direct` is the
    /// word a direct entry produced, which the VM's call does not run again.
    #[inline(always)]
    fn call_helper(&mut self, _stage: &mut S, _site: &Invocation, direct: Option<S::Word>) -> Option<S::Word> {
        direct
    }
}

/// The call of `site`: by the frame's direct entry where it has one, else by the VM. A guard that
/// fails leaves at `miss`, where the VM's call runs.
#[inline(always)]
pub(crate) fn invoke<S: Stage, F: CallFrame<S> + PropertyFrame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss, site: Invocation)
    -> Step<S::Word, F::Control> {
    let direct = match frame.call_entry(&site) {
        CallEntry::Direct(plan) => enter_direct(stage, frame, miss, &site, plan),
        CallEntry::Helper => None,
    };
    frame.call_helper(stage, &site, direct).map_or(Step::Unresolved, Step::Value)
}

/// The receiver `site` binds, a word of the caller's file or the activation's; `None` where the
/// frame cannot read it. A construction has none: its plan allocates it.
#[inline(always)]
fn receiver_word<S: Stage, F: CallFrame<S> + PropertyFrame<S>>(stage: &mut S, frame: &mut F, file: S::Word, miss: S::Miss,
    receiver: Receiver) -> Option<S::Word> {
    match receiver {
        Receiver::Register(register) => Some(stage.load_u64(file, word_offset(register))),
        Receiver::Undefined => Some(stage.constant(Value::UNDEFINED.to_bits())),
        Receiver::This { path: None } => frame.field_this(stage, miss),
        Receiver::This { path: Some((atom, cache_site)) } =>
            get_field(stage, frame, FieldBase::THIS, FieldLookup::Atom { atom, base: FieldBase::THIS, cache_site }, miss),
        Receiver::Allocated => None,
    }
}

/// The callee's word: read and checked once where its identity varies, a constant where the call's
/// form or the frame's proof fixes it, and a method loaded from the receiver through the frame's
/// property rows.
#[inline(always)]
fn callee_word<S: Stage, F: CallFrame<S> + PropertyFrame<S>>(stage: &mut S, frame: &mut F, file: S::Word, miss: S::Miss,
    (site, plan, receiver): (&Invocation, &DirectPlan, Option<S::Word>)) -> Option<S::Word> {
    let held = match (site.callee, plan.identity) {
        (Callee::Register(register), Identity::Guarded(_)) => stage.load_u64(file, word_offset(register)),
        (Callee::Method { atom, site: index }, Identity::Guarded(_)) => frame.call_method(stage, receiver?, atom, index, miss)?,
        (_, identity) => return Some(stage.constant(identity.word())),
    };
    guard_callee(stage, held, plan.identity.word(), miss)?;
    Some(held)
}

/// The callee's `this`: a construction's allocated receiver, or the receiver a call binds, after the
/// callee's identity is proved.
#[inline(always)]
fn callee_this<S: Stage, F: CallFrame<S> + PropertyFrame<S>>(stage: &mut S, frame: &mut F, file: S::Word, miss: S::Miss,
    (site, plan): (&Invocation, &DirectPlan)) -> Option<S::Word> {
    if plan.kind.allocates_receiver() {
        let held = callee_word(stage, frame, file, miss, (site, plan, None))?;
        let allocated = frame.call_receiver(stage, held)?;
        guard_received(stage, allocated, miss)?;
        return Some(allocated);
    }
    let receiver = receiver_word(stage, frame, file, miss, site.receiver)?;
    callee_word(stage, frame, file, miss, (site, plan, Some(receiver)))?;
    bind_this(stage, receiver, plan.sloppy, miss)
}

/// The callee's window at `top`, filled from the call's arguments: a window of the caller's file, or
/// the registers of the list the frame resolves.
#[inline(always)]
fn fill_arguments<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &F, file: S::Word, top: S::Word, (arguments, registers): (Arguments, u16)) -> Option<()> {
    let passed = match arguments {
        Arguments::Window { first, count } => Passed::Window { arguments: stage.add_imm(file, word_bytes(first)), count },
        Arguments::List(list) => Passed::List { file, registers: frame.call_list(list)? },
    };
    fill_call(stage, top, &[], passed, registers);
    Some(())
}

/// The direct call of `site` through `plan`: the call protocol's rows in order, on the words the
/// frame keeps.
#[inline(always)]
fn enter_direct<S: Stage, F: CallFrame<S> + PropertyFrame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss, site: &Invocation,
    plan: DirectPlan) -> Option<S::Word> {
    let constructs = plan.kind.allocates_receiver();
    if constructs != site.constructs() { return None; }
    let file = frame.call_registers(stage)?;
    let this = callee_this(stage, frame, file, miss, (site, &plan))?;
    let room = frame.call_room(stage, miss)?;
    fill_arguments(stage, frame, file, room.top, (site.arguments, plan.registers))?;
    let entered = frame.call_enter(stage, this, room)?;
    let receiver = constructs.then_some(this);
    let value = complete_call(stage, entered.status, entered.word, receiver, plan.returned, entered.unfinished)?;
    frame.call_settle(stage, site, entered, room, value)?;
    Some(value)
}

impl<W, P> Operation<W, P> {
    /// Whether a tier of `facts` lowers the call or return this row is (`Invocation::admits`: a
    /// return's register is one the tier holds, and it completes by status); `None` for any other
    /// row.
    pub(crate) fn admitted(&self, facts: CallFacts) -> Option<bool> {
        match self {
            Self::Call(CallOperation::Site(site)) => Some(site.admits(facts)),
            Self::Return { source } => Some(facts.activates && facts.holds(*source)),
            _ => None,
        }
    }
}

/// Complete the activation with the value of `source`, where the frame can.
#[inline(always)]
pub(crate) fn ret<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, source: Register) -> Step<S::Word, F::Control> {
    let value = frame.tagged(stage, source);
    frame.ret(stage, value).map_or(Step::Unresolved, Step::Control)
}

#[cfg(test)]
mod tests;
