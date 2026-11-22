//! A call and a return as staged operations, written once for every projection. `invoke` is the
//! call protocol (`protocol`) in order: the callee's identity, its `this` (or a construction's
//! receiver), the room both stacks keep, its window, its entry, its completion. The frame holds
//! what its binding time fixed and answers through `CallFrame`:
//!
//! - which entry reaches the callee (`CallFrame::call_entry`, by `plan::plan_entry`): a direct entry
//!   of a published body or a native, whose callee kind the frame folds under the dependency that
//!   retires the caller when it changes and whose identity is checked once per call
//!   (`Identity::Guarded`) or folded the same way (`Identity::Folded`: a known function, a Stable
//!   callee, a Stable bound function unwrapped to its target); or the VM's own call (`CallFrame::call_helper`) for
//!   any callee a site is not bound to;
//! - what the call's form reads (`Invocation`): the callee's register, a function the record names,
//!   or a method its receiver's property rows load; a receiver register, `this` or none; a window
//!   of arguments or a list of registers;
//! - where the words live: the caller's registers, the stacks, the entry itself.
//!
//! The interpreter's call admits through the same rows on machine words (`protocol`'s
//! `concrete_*`); the u-jit's lane frame emits them. `ret` completes an activation with a register.
//! A native's entry is `native_call`.

use super::plan::{CallEntry, CalleeKind, DirectPlan, Identity, NativeId, WordList};
use super::region::Region;
use super::property::get_field;
use super::protocol::{bind_this_with, complete_call, fill_call, guard_callee, guard_callee_word, guard_received};
use super::{word_bytes, word_offset, Arguments, CallFacts, CallOperation, Callee, Frame, Invocation, Operation, Passed, PropertyFrame,
    Receiver, Register, RegisterList, Stage, Step};
#[cfg(not(stencil_build))]
use crate::bytecode::{Op, WideInstruction};
use crate::bytecode::{FieldBase, FieldLookup};
use crate::Value;

/// The general forms of the call-family records a frame may run where its staged row declines
/// (`CallFrame::call_general`): the call's operands are the record's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CallForm {
    /// A call through a callee register and a receiver register, over a window (`Call`, direct eval included).
    Plain,
    /// A call that passes the activation's `arguments` as its last argument (`CallForwardingArguments`).
    Forwarding,
    /// A call whose one argument register holds the array to apply (`CallArray`).
    Array,
    /// A call whose one argument register holds the iterable to spread (`CallSpread`).
    Spread,
    /// A call of a function the record names (`CallKnown`).
    Known,
    /// A method call through its receiver register (`CallMethod`).
    Method,
    /// A method call on the activation's `this` (`CallThisMethod`).
    ThisMethod,
    /// A construction, plain, applied or `super(...)` (`Construct`).
    Construct,
    /// A first-call stub's call of its chunk (`CallFirstChunk`).
    FirstChunk,
}

#[cfg(not(stencil_build))]
/// The general form of the call-family opcode `op`, else `None`: the records the call runner answers
/// (`vm::dispatch`'s `Runner::Call`).
#[cfg(not(stencil_build))]
pub(crate) const fn general_call_form(op: Op) -> Option<CallForm> {
    Some(match op {
        Op::Call => CallForm::Plain,
        Op::CallForwardingArguments => CallForm::Forwarding,
        Op::CallArray => CallForm::Array,
        Op::CallSpread => CallForm::Spread,
        Op::CallKnown => CallForm::Known,
        Op::CallMethod => CallForm::Method,
        Op::CallThisMethod => CallForm::ThisMethod,
        Op::Construct => CallForm::Construct,
        Op::CallFirstChunk => CallForm::FirstChunk,
        _ => return None,
    })
}

/// The stacks a direct call is admitted to: the callee's entry cell, its window at `top` (its
/// end at `next`), and the frame below at `shadow`.
#[derive(Clone, Copy)]
pub struct Room<W> {
    pub cell: W,
    pub top: W,
    pub next: W,
    pub shadow: W,
}

/// A callee's entry: its `status` and result `word`, and where a status other than returned goes.
#[derive(Clone, Copy)]
pub struct Entered<W, M> {
    pub status: W,
    pub word: W,
    pub unfinished: M,
}

/// The call hooks of a frame. Each defaults to a frame with no direct entry, whose every call is
/// the VM's own.
pub trait CallFrame<S: Stage>: Frame<S> {
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

    /// The words of a plan's constant list (a bound function's arguments); `None` where the frame
    /// holds no constants.
    #[inline(always)]
    fn call_constants(&self, _list: WordList) -> Option<&[u64]> {
        None
    }

    /// The method `atom` (method site `site`) of `receiver`, loaded through the frame's property
    /// rows; `None` where the frame has no plan for it, leaving at `miss` a method that is not there.
    #[inline(always)]
    fn call_method(&mut self, _stage: &mut S, _receiver: S::Word, _atom: u32, _site: u32, _miss: S::Miss) -> Option<S::Word> {
        None
    }

    /// The identity the call's callee is checked against, where the frame reads it as a word (an image's
    /// call record, which the loader binds); `None` where the plan's identity is the constant itself.
    #[inline(always)]
    fn call_identity(&mut self, _stage: &mut S) -> Option<S::Word> {
        None
    }

    /// The global object a sloppy callee binds, where the frame reads it as a word (an image's call
    /// record); `None` where the plan's global is the constant itself.
    #[inline(always)]
    fn call_global(&mut self, _stage: &mut S) -> Option<S::Word> {
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

    /// Hand the admitted direct call to the callee's activation in place of entering it
    /// (`call_enter`): the frame fills the callee's window from the site's arguments itself (it
    /// knows how much of it the callee reads before writing), pushes the callee's frame, which holds
    /// `this`, and answers the control the caller's step ends in, which runs the callee's first
    /// record; the callee's completion is its return's, not this step's. `None`, before any effect,
    /// where the frame enters the callee through its cell and takes its completion here
    /// (`call_enter`, `call_settle`). The lane's frame activates (`LaneControl::Resumed`).
    #[inline(always)]
    fn call_activate(&mut self, _stage: &mut S, _this: S::Word, _room: Room<S::Word>) -> Option<Self::Control> {
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

    #[cfg(not(stencil_build))]
    /// Suspend the activation as `suspending` names (`suspend`): the control the step ends in;
    /// `None` where the frame cannot, leaving the suspension to the interpreter. The interpreter's
    /// frame suspends by the step result (`Vm::staged_suspend`), the lane's in place
    /// (`dispatch_fast::lane_frame::suspension_row_in_lane`).
    #[inline(always)]
    fn suspend(&mut self, _stage: &mut S, _suspending: super::Suspending<S::Word>) -> Option<Self::Control> {
        None
    }

    /// Whether the running function is async, a fact of the function the frame runs; `None` where
    /// the frame does not hold it.
    #[inline(always)]
    fn asynchronous(&self) -> Option<bool> {
        None
    }

    /// Whether `value` is an object (`Vm::is_object_like`), which its heap cell decides; `None`
    /// where the frame cannot read the heap.
    #[inline(always)]
    fn object_test(&mut self, _stage: &mut S, _value: S::Word) -> Option<S::Cond> {
        None
    }

    /// PromiseResolve(%Promise%, `value`); `None` where the frame cannot make it, or it raised.
    #[inline(always)]
    fn promise_resolve(&mut self, _stage: &mut S, _value: S::Word) -> Option<S::Word> {
        None
    }

    #[cfg(not(stencil_build))]
    /// The word a step of `yield*`'s delegation defines (`delegation`), with the fact the frame
    /// keeps of it; `None` where the frame cannot make it, or it raised.
    #[inline(always)]
    fn delegation(&mut self, _stage: &mut S, _step: super::DelegationStep<S::Word>) -> Option<(S::Word, Self::Fact)> {
        None
    }

    #[cfg(not(stencil_build))]
    /// The `next` step of a sync `yield*`'s delegate (`DelegationStep::Next`) made by the frame in
    /// place: the control that runs it, whose result arrives in `state` with this instruction run
    /// again (`delegation`). `None`, before any effect, where the frame calls `next` instead.
    #[inline(always)]
    fn delegation_in_place(&mut self, _stage: &mut S, _step: super::DelegationStep<S::Word>, _state: Register)
        -> Option<Self::Control> {
        None
    }

    /// Make the lane's activations frames, as a native that observes the frame stack needs
    /// (`Native::runs_beside_activations` is false): a native that runs beside them is entered
    /// without. The caller's registers stay where they are.
    #[inline(always)]
    fn call_link(&mut self, _stage: &mut S, _miss: S::Miss) -> Option<()> {
        None
    }

    /// Enter `native` with `this` and its `arguments` (a word and a count), a slice of words. A
    /// native that reaches guest code which observes the frames ends with a status other than
    /// returned (the lane's activations left): `unfinished` is where that goes. `None` where the
    /// frame cannot enter natives; it answers `call_result` where it answers this.
    #[inline(always)]
    fn call_native(&mut self, _stage: &mut S, _native: NativeId, _this: S::Word, _arguments: (S::Word, u16), _miss: S::Miss)
        -> Option<Entered<S::Word, S::Miss>> {
        None
    }

    /// The native returned `value`: define the call's result.
    #[inline(always)]
    fn call_result(&mut self, _stage: &mut S, _site: &Invocation, _value: S::Word) -> Option<()> {
        None
    }

    /// The VM's own call of `site`, which a call that took no direct entry runs; `direct` is the
    /// word a direct entry produced, which the VM's call does not run again.
    #[inline(always)]
    fn call_helper(&mut self, _stage: &mut S, _site: &Invocation, direct: Option<S::Word>) -> Option<S::Word> {
        direct
    }

    #[cfg(not(stencil_build))]
    /// The general form `form` of the call-family record `instruction` at resume PC `pc`, which the
    /// call row runner asks where the row declined (the record's operands are read from it, since an
    /// `Invocation` does not name every form). `None` where the frame runs no general calls.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    fn call_general(&mut self, _form: CallForm, _instruction: WideInstruction, _pc: usize) -> Option<Step<S::Word, Self::Control>> {
        None
    }
}

/// The call of `site`: by the frame's direct entry where it has one (a unit's inlined callee is a
/// `CallEntry::Inline` the unit spliced, whose entry stays the fallback), else by the VM. A
/// guard that fails leaves at `miss`, where the VM's call runs.
#[inline(always)]
pub(crate) fn invoke<S: Stage, F: CallFrame<S> + PropertyFrame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss, site: Invocation)
    -> Step<S::Word, F::Control> {
    let direct = match frame.call_entry(&site) {
        CallEntry::Direct(plan) | CallEntry::Inline(plan) => match plan.kind {
            CalleeKind::Native(native) => super::native_call::enter_native(stage, frame, miss, &site, (plan, native)),
            CalleeKind::Script | CalleeKind::Class => match enter_direct(stage, frame, miss, &site, plan) {
                Some(Direct::Activated(control)) => return Step::Control(control),
                Some(Direct::Returned(word)) => Some(word),
                None => None,
            },
        },
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
        Receiver::Register(register) => Some(stage.load_u64(Region::Frame.mutable(), file, word_offset(register))),
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
        (Callee::Register(register), Identity::Guarded(_)) => stage.load_u64(Region::Frame.mutable(), file, word_offset(register)),
        (Callee::Method { atom, site: index }, Identity::Guarded(_)) => frame.call_method(stage, receiver?, atom, index, miss)?,
        (_, identity) => return Some(frame.call_identity(stage).unwrap_or_else(|| stage.constant(identity.word()))),
    };
    match frame.call_identity(stage) {
        Some(expected) => guard_callee_word(stage, held, expected, miss)?,
        None => guard_callee(stage, held, plan.identity.word(), miss)?,
    }
    Some(held)
}

/// The callee's `this`: a construction's allocated receiver, or the receiver a call binds (a bound
/// function's own), after the callee's identity is proved.
#[inline(always)]
pub(super) fn callee_this<S: Stage, F: CallFrame<S> + PropertyFrame<S>>(stage: &mut S, frame: &mut F, file: S::Word, miss: S::Miss,
    (site, plan): (&Invocation, &DirectPlan)) -> Option<S::Word> {
    if plan.kind.allocates_receiver() {
        let held = callee_word(stage, frame, file, miss, (site, plan, None))?;
        let allocated = frame.call_receiver(stage, held)?;
        guard_received(stage, allocated, miss)?;
        return Some(allocated);
    }
    let receiver = receiver_word(stage, frame, file, miss, site.receiver)?;
    callee_word(stage, frame, file, miss, (site, plan, Some(receiver)))?;
    let this = plan.bound.map_or(receiver, |bound| stage.constant(bound.this));
    let global = plan.sloppy.map(|global| move |stage: &mut S| frame.call_global(stage).unwrap_or_else(|| stage.constant(global)));
    bind_this_with(stage, this, global, miss)
}

/// The callee's window at `top` of `registers` words, filled from the bound function's arguments
/// the plan folded, then the call's: a window of the caller's file, or the registers of the list
/// the frame resolves.
#[inline(always)]
pub(super) fn fill_arguments<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &F, file: S::Word, top: S::Word,
    (site, plan, registers): (&Invocation, &DirectPlan, u16)) -> Option<()> {
    let passed = match site.arguments {
        Arguments::Window { first, count } => Passed::Window { arguments: stage.add_imm(file, word_bytes(first)), count },
        Arguments::List(list) => Passed::List { file, registers: frame.call_list(list)? },
    };
    let prefix: &[u64] = match plan.bound {
        Some(bound) => frame.call_constants(bound.arguments)?,
        None => &[],
    };
    fill_call(stage, top, prefix, passed, registers);
    Some(())
}

/// How a direct call ended its step: the callee returned the word, or the frame handed the call to
/// the callee's activation (`CallFrame::call_activate`), whose control the step ends in.
enum Direct<W, C> {
    Returned(W),
    Activated(C),
}

/// The direct call of `site` through `plan`: the call protocol's rows in order, on the words the
/// frame keeps.
#[inline(always)]
fn enter_direct<S: Stage, F: CallFrame<S> + PropertyFrame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss, site: &Invocation,
    plan: DirectPlan) -> Option<Direct<S::Word, F::Control>> {
    let constructs = plan.kind.allocates_receiver();
    if constructs != site.constructs() || (constructs && plan.bound.is_some()) { return None; }
    let file = frame.call_registers(stage)?;
    let this = callee_this(stage, frame, file, miss, (site, &plan))?;
    let room = frame.call_room(stage, miss)?;
    if let Some(control) = frame.call_activate(stage, this, room) {
        return Some(Direct::Activated(control));
    }
    fill_arguments(stage, frame, file, room.top, (site, &plan, plan.registers))?;
    let entered = frame.call_enter(stage, this, room)?;
    let receiver = constructs.then_some(this);
    let value = complete_call(stage, entered.status, entered.word, receiver, plan.returned, entered.unfinished)?;
    frame.call_settle(stage, site, entered, room, value)?;
    Some(Direct::Returned(value))
}

impl<W, P> Operation<W, P> {
    /// Whether a tier of `facts` lowers the call or return this row is (`Invocation::admits`: a
    /// return's register is one the tier holds, and it completes by status); `None` for any other
    /// row.
    pub fn admitted(&self, facts: CallFacts) -> Option<bool> {
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
    frame.ret(stage, source).map_or(Step::Unresolved, Step::Control)
}

#[cfg(test)]
mod tests;
