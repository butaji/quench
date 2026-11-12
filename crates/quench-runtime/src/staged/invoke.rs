//! A call and a return as staged operations, written once for every projection. `invoke` is the
//! call protocol (`protocol`) in order: the callee's identity, its `this` (or a construction's
//! receiver), the room both stacks keep, its window, its entry, its completion. The frame holds
//! what its binding time fixed and answers through `CallFrame`:
//!
//! - which entry reaches the callee (`CallFrame::call_entry`): a direct entry of a published body,
//!   whose callee kind and identity the frame folds under the dependency that retires the caller
//!   when either changes (`DirectPlan`), or the VM's own call (`CallFrame::call_helper`) for any
//!   callee a site is not bound to: a native, a bound function, a callee whose identity changed;
//! - where the words live: the caller's registers, the stacks, the entry itself.
//!
//! The interpreter's call admits through the same rows on machine words (`protocol`'s
//! `concrete_*`); the u-jit's lane frame emits them. `ret` completes an activation with a register.

use super::protocol::{bind_this, complete_call, fill_window, guard_callee, guard_received, window_offset};
use super::{CallFacts, CallOperation, Frame, Invocation, Operation, Register, Stage, Step, WindowFill};

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

/// What a frame knows of a site's callee at the time it is compiled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct DirectPlan {
    pub(crate) kind: CalleeKind,
    /// The callee's identity word, which the site was bound to.
    pub(crate) identity: u64,
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
pub(crate) fn invoke<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss, site: Invocation)
    -> Step<S::Word, F::Control> {
    let direct = match frame.call_entry(&site) {
        CallEntry::Direct(plan) => enter_direct(stage, frame, miss, &site, plan),
        CallEntry::Helper => None,
    };
    frame.call_helper(stage, &site, direct).map_or(Step::Unresolved, Step::Value)
}

/// The direct call of `site` through `plan`: the call protocol's rows in order, on the words the
/// frame keeps.
#[inline(always)]
fn enter_direct<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss, site: &Invocation, plan: DirectPlan)
    -> Option<S::Word> {
    let file = frame.call_registers(stage)?;
    let held = stage.load_u64(file, window_offset(site.callee));
    guard_callee(stage, held, plan.identity, miss)?;
    let this = if plan.kind.allocates_receiver() {
        let allocated = frame.call_receiver(stage, held)?;
        guard_received(stage, allocated, miss)?;
        allocated
    } else {
        let receiver = stage.load_u64(file, window_offset(site.receiver?));
        bind_this(stage, receiver, plan.sloppy, miss)?
    };
    let room = frame.call_room(stage, miss)?;
    fill_window(stage, room.top, file, WindowFill { first: site.window, argc: site.argc, registers: plan.registers });
    let entered = frame.call_enter(stage, this, room)?;
    let receiver = plan.kind.allocates_receiver().then_some(this);
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
mod tests {
    use super::*;
    use crate::staged::{Concrete, Edge};
    use crate::Value;

    const RETURNED: u32 = 3;
    const IDENTITY: u64 = 0x1000;
    const GLOBAL: u64 = 0x2000;

    /// A frame over plain memory: a register file, one stack of words, and a callee that returns
    /// the sum of its window.
    struct Probe {
        file: [u64; 8],
        stack: [u64; 8],
        entry: CallEntry,
        entered: Vec<u64>,
        helper: Option<u64>,
        settled: Option<u64>,
        returned: Option<u64>,
    }

    impl Probe {
        fn new(entry: CallEntry) -> Self {
            Self { file: [0; 8], stack: [0; 8], entry, entered: Vec::new(), helper: None, settled: None, returned: None }
        }
    }

    impl Frame<Concrete> for Probe {
        type Fact = ();
        type Control = Edge;
        fn fact(&self, _register: Register) {}
        fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 { self.file[usize::from(register)] }
        fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: ()) {
            self.file[usize::from(register)] = value;
        }
        fn this(&mut self, _stage: &mut Concrete) -> u64 { Value::DELETED.to_bits() }
        fn define_this(&mut self, _stage: &mut Concrete, _destination: Register, _this: u64) {}
        fn constant(&mut self, _stage: &mut Concrete, constant: crate::staged::Constant) -> (u64, ()) {
            (constant.value.to_bits(), ())
        }
        fn truthiness(&mut self, _stage: &mut Concrete, _register: Register) -> Option<bool> { None }
        fn branch(&mut self, _stage: &mut Concrete, _condition: bool) -> Edge { Edge::Following }
        fn jump(&mut self, _stage: &mut Concrete) -> Edge { Edge::Target }
        fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) { (Value::boolean(condition).to_bits(), ()) }
        fn ret(&mut self, _stage: &mut Concrete, value: u64) -> Option<Edge> {
            self.returned = Some(value);
            Some(Edge::Target)
        }
    }

    impl CallFrame<Concrete> for Probe {
        fn call_entry(&self, _site: &Invocation) -> CallEntry { self.entry }
        fn call_registers(&mut self, _stage: &mut Concrete) -> Option<u64> { Some(self.file.as_ptr() as u64) }
        fn call_receiver(&mut self, _stage: &mut Concrete, _constructor: u64) -> Option<u64> { Some(Value::integer(77).to_bits()) }
        fn call_room(&mut self, _stage: &mut Concrete, _miss: ()) -> Option<Room<u64>> {
            let top = self.stack.as_mut_ptr() as u64;
            Some(Room { cell: 0, top, next: top, shadow: 0 })
        }
        fn call_enter(&mut self, _stage: &mut Concrete, this: u64, _room: Room<u64>) -> Option<Entered<u64, ()>> {
            self.entered = self.stack[..3].to_vec();
            Some(Entered { status: u64::from(RETURNED), word: this, unfinished: () })
        }
        fn call_settle(&mut self, _stage: &mut Concrete, _site: &Invocation, _entered: Entered<u64, ()>, _room: Room<u64>,
            value: u64) -> Option<()> {
            self.settled = Some(value);
            Some(())
        }
        fn call_helper(&mut self, _stage: &mut Concrete, _site: &Invocation, direct: Option<u64>) -> Option<u64> {
            direct.or_else(|| { self.helper = Some(1); Some(0) })
        }
    }

    fn plan(kind: CalleeKind, sloppy: Option<u64>) -> CallEntry {
        CallEntry::Direct(DirectPlan { kind, identity: IDENTITY, sloppy, registers: 3, returned: RETURNED })
    }

    fn site(receiver: Option<Register>) -> Invocation {
        Invocation { callee: 0, receiver, window: 2, argc: 2, result: 7, returns: false, forwards: false }
    }

    fn call(probe: &mut Probe, site: Invocation) -> Option<u64> {
        match invoke(&mut Concrete::new(std::ptr::null()), probe, (), site) {
            Step::Value(word) => Some(word),
            _ => None,
        }
    }

    #[test]
    fn a_frame_without_a_direct_entry_takes_the_vm_call() {
        let mut probe = Probe::new(CallEntry::Helper);
        assert_eq!(call(&mut probe, site(Some(1))), Some(0));
        assert_eq!((probe.helper, probe.settled, probe.entered.len()), (Some(1), None, 0));
    }

    #[test]
    fn a_direct_call_admits_binds_fills_enters_and_settles_in_order() {
        let mut probe = Probe::new(plan(CalleeKind::Script, Some(GLOBAL)));
        let receiver = Value::UNDEFINED.to_bits();
        probe.file[..5].copy_from_slice(&[IDENTITY, receiver, 10, 20, 0]);
        assert_eq!(call(&mut probe, site(Some(1))), Some(GLOBAL), "a sloppy callee binds the global for undefined");
        assert_eq!(probe.entered, [10, 20, Value::UNDEFINED.to_bits()], "the window takes the arguments then undefined");
        assert_eq!((probe.settled, probe.helper), (Some(GLOBAL), None));
    }

    #[test]
    fn a_construction_yields_the_receiver_its_plan_allocated() {
        let mut probe = Probe::new(plan(CalleeKind::Class, None));
        probe.file[..4].copy_from_slice(&[IDENTITY, 0, 5, 6]);
        assert_eq!(call(&mut probe, site(None)), Some(Value::integer(77).to_bits()));
        assert_eq!(probe.entered[..2], [5, 6]);
    }

    #[test]
    fn a_callee_that_is_not_the_bound_one_takes_the_vm_call() {
        let mut probe = Probe::new(plan(CalleeKind::Script, None));
        probe.file[0] = IDENTITY + 1;
        assert_eq!(call(&mut probe, site(Some(1))), Some(0));
        assert_eq!((probe.helper, probe.entered.len()), (Some(1), 0), "nothing entered");
    }

    #[test]
    fn a_sloppy_callee_leaves_a_primitive_receiver_to_the_host() {
        let mut probe = Probe::new(plan(CalleeKind::Script, Some(GLOBAL)));
        probe.file[..2].copy_from_slice(&[IDENTITY, Value::integer(5).to_bits()]);
        assert_eq!(call(&mut probe, site(Some(1))), Some(0));
        assert_eq!(probe.helper, Some(1));
    }

    #[test]
    fn a_return_completes_with_the_register_value() {
        let mut probe = Probe::new(CallEntry::Helper);
        probe.file[3] = 42;
        let step = ret(&mut Concrete::new(std::ptr::null()), &mut probe, 3);
        assert!(matches!(step, Step::Control(Edge::Target)));
        assert_eq!(probe.returned, Some(42));
    }
}
