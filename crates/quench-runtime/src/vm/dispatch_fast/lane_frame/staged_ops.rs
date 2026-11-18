//! The staged operations the lane runs over a `LaneFrame`: each builds the frame of the running
//! record, steps the operation on the concrete stage, and answers the edge or the written value.
use super::*;



/// `LoadCapture` of the capture at `depth` and `slot` into `destination` (of the root's projected
/// binding where `root`), by the staged operation: the running function's own slots, else the lane's
/// remembered walks; false, writing nothing, where they hold no such value.
/// The row runs first on a frame without the lane's reads: an own slot needs none, and with no
/// reads to call nothing of the frame escapes, so it stays in registers. The remembered walks
/// run the row again with them (`staged_load_capture_walked`), out of line.
#[inline(always)]
pub(in crate::vm::dispatch_fast) fn staged_load_capture<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    destination: u16, at: (u16, u16), root: bool) -> bool {
    load_capture_with::<H>(cx, r, view, (destination, at, root), Reads::NONE)
        || staged_load_capture_walked::<H>(cx, r, view, destination, at, root)
}

/// `staged_load_capture` through the lane's remembered walks (`Reads::of_captures`).
#[cold]
#[inline(never)]
fn staged_load_capture_walked<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    destination: u16, at: (u16, u16), root: bool) -> bool {
    load_capture_with::<H>(cx, r, view, (destination, at, root), Reads::of_captures::<H>(cx))
}

/// The `LoadCapture` row of `destination`, capture `(depth, slot)` and `root` over the running
/// function's own slots and `reads`.
#[inline(always)]
fn load_capture_with<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    (destination, (depth, slot), root): (u16, (u16, u16), bool), reads: Reads) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    frame.capture_slots = Some(capture_reads(unsafe { (*cx).registers }.capture()));
    frame.reads = reads;
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::LoadCapture { destination, depth, slot, root }), Step::Value(_))
}

/// What the staged `await` conversion (`staged::await_source`) asks the lane context, erased of
/// its host so the frame names no `H`: whether a value is an object, and PromiseResolve where it
/// observes nothing (`Vm::lane_promise_resolve`).
#[derive(Clone, Copy)]
pub(in crate::vm::dispatch_fast) struct AwaitSource {
    object_like: fn(*mut (), Value) -> bool,
    resolve: fn(*mut (), Value) -> Option<Value>,
}

impl AwaitSource {
    /// The conversion of a lane context of host `H`.
    pub(in crate::vm::dispatch_fast) fn of<H: Host>() -> Self {
        Self {
            object_like: |context, value| unsafe { vm(context.cast::<LaneContext<H>>()) }.is_object_like(value),
            resolve: |context, value| {
                let cx = context.cast::<LaneContext<H>>();
                let vm = unsafe { vm(cx) };
                let promise = vm.intrinsic_promise_constructor();
                // SAFETY: the context's program outlives the lane run.
                vm.promise_resolve_in_lane(unsafe { &*(*cx).code }, promise, value)
            },
        }
    }
}

impl<const ACC: u8> crate::staged::CallFrame<Concrete> for LaneFrame<ACC> {
    /// Whether `value` is an object, by the lane context's VM; `None` without a context.
    fn object_like(&mut self, _stage: &mut Concrete, value: u64) -> Option<bool> {
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

    /// The row's suspension made in place (`LaneSuspends::suspend`), which the handler chose
    /// before the row ran, so it never declines after the row's effects.
    fn suspend(&mut self, _stage: &mut Concrete, suspending: crate::staged::Suspending<u64>) -> Option<LaneControl> {
        self.suspends.map(|suspends| LaneControl::Resumed(suspends.suspend(suspending)))
    }

    /// The delegation's reads the lane finishes without guest code (`LaneSuspends::delegation`).
    fn delegation(&mut self, _stage: &mut Concrete, step: crate::staged::DelegationStep<u64>) -> Option<(u64, ())> {
        self.suspends?.delegation(step).map(|word| (word, ()))
    }

    /// A delegate generator's step made in place (`LaneSuspends::in_place`).
    fn delegation_in_place(&mut self, _stage: &mut Concrete, step: crate::staged::DelegationStep<u64>, _state: Register)
        -> Option<LaneControl> {
        self.suspends?.in_place(self.registers, step).map(LaneControl::Resumed)
    }
}

/// What `await` of `awaited` waits on in the lane context `cx` (registers `r`): the staged
/// conversion (`staged::await_source`) over the lane's own PromiseResolve. `None` leaves a
/// conversion that could run guest code, or throw into the frame's handlers, to the general path.
pub(in crate::vm::dispatch_fast) fn lane_await_source<H: Host>(cx: *mut LaneContext<H>, r: Registers, awaited: Value) -> Option<Value> {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::hosted::<H>(r, Carried::script(cx), cx);
    crate::staged::await_source(&mut stage, &mut frame, awaited.to_bits()).map(Value::from_bits)
}
