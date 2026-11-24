//! The staged operations the lane runs over a `LaneFrame`: each builds the frame of the running
//! record, steps the operation on the concrete stage, and answers the edge or the written value.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;


/// `LoadCapture` of the capture at `depth` and `slot` into `destination` (of the root's projected
/// binding where `root`), by the staged operation: the running function's own slots, else the lane's
/// remembered walks; false, writing nothing, where they hold no such value.
/// The row runs first on a frame without the lane's reads: an own slot needs none, and with no
/// reads to call nothing of the frame escapes, so it stays in registers. The remembered walks
/// run the row again with them (`staged_load_capture_walked`), out of line.
#[inline(always)]
pub(crate) fn staged_load_capture(cx: *mut LaneContext, r: Registers, view: Carried,
    destination: u16, at: (u16, u16), root: bool) -> bool {
    load_capture_with(cx, r, view, (destination, at, root), Reads::NONE)
        || staged_load_capture_walked(cx, r, view, destination, at, root)
}

/// `staged_load_capture` through the lane's remembered walks (`Reads::of_captures`).
#[cold]
#[inline(never)]
fn staged_load_capture_walked(cx: *mut LaneContext, r: Registers, view: Carried,
    destination: u16, at: (u16, u16), root: bool) -> bool {
    load_capture_with(cx, r, view, (destination, at, root), Reads::of_captures(cx))
}

/// The `LoadCapture` row of `destination`, capture `(depth, slot)` and `root` over the running
/// function's own slots and `reads`.
#[inline(always)]
fn load_capture_with(cx: *mut LaneContext, r: Registers, view: Carried,
    (destination, (depth, slot), root): (u16, (u16, u16), bool), reads: Reads) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    frame.capture_slots = Some(capture_reads(unsafe { (*cx).registers }.capture()));
    frame.reads = reads;
    matches!(quench_runtime::staged::step_of::<{ quench_runtime::staged::op_kind::LOAD_CAPTURE }, _, _>(&mut stage, &mut frame, Operation::LoadCapture { destination, depth, slot, root }), Step::Value(_))
}

/// What the staged `await` conversion (`staged::await_source`) asks the lane context, erased of
/// its host so the frame names no `H`: whether a value is an object, and PromiseResolve where it
/// observes nothing (`Vm::intrinsic_promise_resolve`).
#[derive(Clone, Copy)]
pub(crate) struct AwaitSource {
    object_like: fn(*mut (), Value) -> bool,
    resolve: fn(*mut (), Value) -> Option<Value>,
}

impl AwaitSource {
    /// The conversion of a lane context of host `H`.
    #[inline(always)]
    pub(crate) fn of() -> Self {
        Self {
            object_like: |context, value| unsafe { vm(context.cast::<LaneContext>()) }.is_object_like(value),
            resolve: |context, value| {
                let cx = context.cast::<LaneContext>();
                let vm = unsafe { vm(cx) };
                // SAFETY: the context's program outlives the lane run.
                vm.intrinsic_promise_resolve(unsafe { &*(*cx).code }, value)
            },
        }
    }
}

/// The activation entries of the running lane context, erased of its host so the frame names no `H`
/// (`PrologueFrame`): the tier prologue and the body entry the prologue records run
/// (`tier_prologue_resumed`, `body_entry_resumed`), each answering where the lane resumes. `ip` is the
/// body entry's record.
#[derive(Clone, Copy)]
pub(crate) struct EntryHost {
    pub(crate) cx: *mut (),
    pub(crate) ip: super::super::Ip,
    pub(crate) tier: unsafe fn(*mut (), Registers, Carried, u32, usize) -> super::super::Resumed,
    pub(crate) body: unsafe fn(*mut (), super::super::Ip, Registers, Carried, u32, bool) -> super::super::Resumed,
}

impl<const ACC: u8> quench_runtime::staged::PrologueFrame<Concrete> for LaneFrame<ACC> {
    /// The tier prologue's entry: the function is entered in the lane context, which resumes where
    /// the activation continues (`tier_prologue_resumed`).
    fn tier_prologue(&mut self, _stage: &mut Concrete, function: u32, parameters: u16) -> Option<LaneControl> {
        let entry = self.entry?;
        // SAFETY: the row runs in the lane context `entry` names, whose views outlive the run.
        let resumed = unsafe { (entry.tier)(entry.cx, self.registers, self.view, function, usize::from(parameters)) };
        Some(LaneControl::Resumed(resumed))
    }

    /// The specialized body's prologue: the body runs on the activation's registers, and the lane
    /// resumes where it leaves (`body_entry_resumed`).
    fn body_entry(&mut self, _stage: &mut Concrete, function: u32, binds_this: bool) -> Option<LaneControl> {
        let entry = self.entry?;
        // SAFETY: as `tier_prologue`; `entry.ip` is the prologue record the row runs at.
        let resumed = unsafe { (entry.body)(entry.cx, entry.ip, self.registers, self.view, function, binds_this) };
        Some(LaneControl::Resumed(resumed))
    }
}

/// The plan of `call`'s call to `target`, its `this` bound as `direct_call_this_in_lane` binds it: lexical
/// (and plain) as the bound `this`, sloppy through `bind_this` with the global object, else as it is.
#[inline(always)]
fn direct_plan(call: &LaneCall, target: &ScriptCallTarget) -> quench_runtime::staged::CallEntry {
    let lexical = || Some(quench_runtime::staged::plan::Bound { this: target.lexical_this.to_bits(),
        arguments: quench_runtime::staged::plan::WordList { start: 0, count: 0 } });
    let (sloppy, bound) = if call.form.plain {
        // A plain call's receiver is `undefined` by its record, whose `this` is its target's.
        (None, lexical())
    } else {
        match target.this_binding {
            ThisBinding::Lexical => (None, lexical()),
            // SAFETY: the realm's global object, read only for a sloppy callee.
            ThisBinding::Sloppy => (Some(unsafe { *call.globals }.to_bits()), None),
            _ => (None, None),
        }
    };
    quench_runtime::staged::CallEntry::Direct(quench_runtime::staged::DirectPlan {
        kind: quench_runtime::staged::CalleeKind::Script,
        identity: quench_runtime::staged::Identity::Guarded(target.callee.to_bits()),
        sloppy,
        registers: target.registers,
        returned: 0,
        bound,
    })
}

/// A getter's entry (`getters::invoke_getter`), a plain call where it is one: a read that writes its register,
/// no `with` scope, an entry that activates its registers alone, of the running program; else `Helper`.
#[inline(always)]
fn getter_entry(call: &LaneCall, site: &quench_runtime::staged::Invocation) -> quench_runtime::staged::CallEntry {
    let cx = call.context.cast::<LaneContext>();
    // SAFETY: the running lane's context; `target` is the entry the read resolved, which outlives the call.
    let (vm, target) = unsafe { (vm(cx), &*call.target.get()) };
    let returns = unsafe { (*call.record).a } & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if site.returns || returns || vm.with_scope_entered || !target.entry.activates() || target.forwards() || target.settles()
        || target.program() != unsafe { (*cx).program } {
        return quench_runtime::staged::CallEntry::Helper;
    }
    direct_plan(call, target)
}

/// The call hooks of a call row's frame (`protocol_row`, `LaneCall`): the proved plain call
/// (`script_call_miss`'s proven arm, `lane_plain_activation!`) and the proved leaf call
/// (`script_call_leaf_miss`) as the staged call protocol's rows (`staged::invoke`), in its order:
///
/// - `call_entry`: the site's entry, the link masked to the cache (no bounds test), tested whole by
///   one admission compare (link-proofs.md: generation, program, the run's conditions, `DIAGNOSED`,
///   `UNPROVEN`), the argument count against its parameters, and a leaf row's form; the callee's
///   identity is the plan's guard (`Identity::Guarded`), the one compare `invoke` makes of it;
/// - `this`: the plan binds as `direct_call_this_in_lane` does: an arrow's lexical `this` (and a plain
///   receiver's, `PLAIN`) as the plan's bound `this`, a sloppy callee's through `bind_this` with the
///   realm's global object, any other the receiver as it is;
/// - `call_room`: the one `plain_room` compare, at the plain depth it loads once;
/// - `call_activate`: the window filled and the plain header pushed (`enter_plain_window`,
///   `protocol::push_plain_at`) or the leaf entered (`lane_run_leaf`), handed back as
///   `LaneControl::Resumed`.
///
/// Anything else (another form, a returning call, a failed compare or binding, no room) answers
/// `CallEntry::Helper` or leaves at the guard before any word is written, so the row is
/// `Step::Unresolved` and its miss arm (`protocol_miss`) runs the full handler.
impl<const ACC: u8> quench_runtime::staged::CallFrame<Concrete> for LaneFrame<ACC> {
    /// Whether `value` is an object, by the lane context's VM; `None` without a context.
    fn object_test(&mut self, _stage: &mut Concrete, value: u64) -> Option<bool> {
        // A value outside the heap is no object; only a cell asks the lane context.
        if !Value::from_bits(value).is_heap() {
            return Some(false);
        }
        let host = self.host?;
        Some((host.awaits.object_like)(host.cx, Value::from_bits(value)))
    }

    /// PromiseResolve(%Promise%, `value`) where it observes nothing; `None` leaves a resolution
    /// that could run guest code, or throw into the frame's handlers, to the general path.
    fn promise_resolve(&mut self, _stage: &mut Concrete, value: u64) -> Option<u64> {
        let host = self.host?;
        (host.awaits.resolve)(host.cx, Value::from_bits(value)).map(Value::to_bits)
    }

    /// Whether the activation that suspends in place is async; `None` on a frame that suspends nothing.
    fn asynchronous(&self) -> Option<bool> {
        self.suspends.map(LaneSuspends::asynchronous)
    }

    /// The row's suspension, which the handler makes in place by the suspender it chose before
    /// the row ran (`suspension_row_in_lane`), so it never declines after the row's effects.
    fn suspend(&mut self, _stage: &mut Concrete, suspending: quench_runtime::staged::Suspending<u64>) -> Option<LaneControl> {
        self.suspends.map(|_| LaneControl::Suspend(suspending))
    }

    /// The delegation's reads the lane finishes without guest code (`LaneSuspends::delegation`).
    fn delegation(&mut self, _stage: &mut Concrete, step: quench_runtime::staged::DelegationStep<u64>) -> Option<(u64, Boxing)> {
        self.suspends?.delegation(step).map(|word| (word, Boxing::Value))
    }

    /// A delegate generator's step made in place (`LaneSuspends::in_place`).
    fn delegation_in_place(&mut self, _stage: &mut Concrete, step: quench_runtime::staged::DelegationStep<u64>, _state: Register)
        -> Option<LaneControl> {
        self.suspends?.in_place(self.registers, step).map(LaneControl::Resumed)
    }

    #[inline(always)]
    fn call_entry(&self, site: &quench_runtime::staged::Invocation) -> quench_runtime::staged::CallEntry {
        use quench_runtime::staged::{Arguments, Callee, Receiver};
        let Some(call) = &self.call else { return quench_runtime::staged::CallEntry::Helper };
        if site.receiver == Receiver::Base {
            return getter_entry(call, site);
        }
        // The form the proved calls make: a callee and a receiver register and a window, a
        // result defined, not returned (a returning call's admission is the run's,
        // `returning_call_admitted`, which the miss arm makes).
        let (Callee::Register(_), Receiver::Register(_), Arguments::Window { count, .. }) = (site.callee, site.receiver, site.arguments) else {
            return quench_runtime::staged::CallEntry::Helper;
        };
        if site.returns || site.forwards {
            return quench_runtime::staged::CallEntry::Helper;
        }
        // SAFETY: the running lane's stack, which nothing below changes.
        let stack = unsafe { &*call.stack };
        let entries = &stack.script_targets.entries;
        // SAFETY: the call record the row runs.
        let index = usize::from(unsafe { (*call.record).call_link.get() }) & (entries.len() - 1);
        // SAFETY: the cache's length is a power of two (`SCRIPT_CALL_TARGETS_INITIAL`), so the masked
        // index lies within it; nothing before the hand-off resolves a target or collects.
        let target = unsafe { entries.get_unchecked(index) };
        // A static count is the row's (`call_form`), so its tests fold as `script_call_miss`'s `ARGC`.
        let argc = if call.form.argc == ANY_ARGC { count } else { call.form.argc };
        let leaf = call.form.leaf;
        if target.admission != stack.admission
            || (leaf == NOT_LEAF && argc > FIXED_ARGUMENT_COPIES)
            || (leaf != NOT_LEAF && target.entry != CallEntry::leaf(leaf))
            || !quench_runtime::staged::call::concrete_fits(u64::from(target.params), u64::from(argc))
        {
            return quench_runtime::staged::CallEntry::Helper;
        }
        debug_assert!(leaf != NOT_LEAF || target.entry.activates(), "a proved plain call's entry activates");
        call.target.set(std::ptr::from_ref(target));
        direct_plan(call, target)
    }

    /// A getter's base (`Receiver::Base`).
    #[inline(always)]
    fn call_base(&mut self, _stage: &mut Concrete) -> Option<u64> {
        Some(self.call.as_ref()?.base.to_bits())
    }

    #[inline(always)]
    fn call_registers(&mut self, _stage: &mut Concrete) -> Option<u64> {
        self.call.as_ref()?;
        Some(self.registers.0.expose_provenance() as u64)
    }

    /// A method site's list (`Arguments::List`, `CallMethod`/`CallThisMethod` only): no lane
    /// handler runs those records (`other_handler` names neither), so no call row reaches here. A
    /// method call `o.m(x)` is a register `Call` whose receiver is `c`, which `call_entry` admits.
    #[inline(always)]
    fn call_list(&self, _list: quench_runtime::staged::RegisterList) -> Option<&[Register]> {
        None
    }

    /// The constants of the plan `call_entry` made: only the empty list of its lexical `this`
    /// (`Bound`), which prefixes no argument.
    #[inline(always)]
    fn call_constants(&self, list: quench_runtime::staged::plan::WordList) -> Option<&[u64]> {
        (self.call.is_some() && list.count == 0).then_some(&[][..])
    }

    /// A method site's load (`Callee::Method`): as `call_list`, no call row runs a method site.
    #[inline(always)]
    fn call_method(&mut self, _stage: &mut Concrete, _receiver: u64, _atom: u32, _site: u32, _miss: ()) -> Option<u64> {
        None
    }

    /// A construction's receiver: no call row runs a construction (`Construct` takes
    /// `lane_script_construct`, which requickens to the planned constructions; `CONSTRUCT` has no
    /// protocol row), and `call_entry` answers `Helper` for `Receiver::Allocated`.
    #[inline(always)]
    fn call_receiver(&mut self, _stage: &mut Concrete, _constructor: u64) -> Option<u64> {
        None
    }

    #[inline(always)]
    fn call_room(&mut self, _stage: &mut Concrete, _miss: ()) -> Option<quench_runtime::staged::Room<u64>> {
        let call = self.call.as_mut()?;
        // SAFETY: the running lane's stack and the entry `call_entry` admitted.
        let (stack, target) = unsafe { (&*call.stack, &*call.target.get()) };
        // The activation count and the values' room in one compare, the depth loaded once for the
        // push past it (`LaneStack::plain_room`); a leaf pushes none, so the bound stands for it too.
        let depth = stack.plain.depth.get();
        if !stack.plain_room(depth, target.room) {
            return None;
        }
        call.depth = depth;
        let top = stack.values.top().expose_provenance() as u64;
        Some(quench_runtime::staged::Room { cell: 0, top, next: top, shadow: 0 })
    }

    #[inline(always)]
    fn call_activate(&mut self, _stage: &mut Concrete, this: u64, _room: quench_runtime::staged::Room<u64>) -> Option<LaneControl> {
        let call = self.call.as_ref()?;
        // SAFETY: the hand-off's contract (`CallActivation`): the context, record and registers are
        // the running call's, and the entry, its room at `depth` and `this` what the hooks before
        // admitted and bound.
        let (registers, start) = unsafe {
            (call.activate)(call.context, self.registers, call.ip, call.record, call.target.get(), call.depth, Value::from_bits(this))
        };
        // The callee's view is the caller's script view: the same context, no accumulator.
        Some(LaneControl::Resumed(Resumed::At(registers, start, Carried { acc: Slot::unset(), ..self.view })))
    }
}
