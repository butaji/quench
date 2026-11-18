//! The `with` scope chain of a lane activation: data on the lane stack, as a frame's is on the
//! VM's scope stack. An activation's chain is its closure's captured objects then the ones it
//! entered, outermost first, each tagged with the activation that owns it
//! (`LaneStack::with_objects`); a suspended frame's record keeps it while no activation runs
//! the frame (`FrameRecord::with_objects`), and a materialized activation's frame takes it.
use super::*;

/// The objects of one activation's `with` chain, kept inline up to the nesting a function usually has.
pub(super) type WithChain = SmallVec<[Value; WITH_CHAIN_INLINE_OBJECTS]>;
const WITH_CHAIN_INLINE_OBJECTS: usize = 2;

impl LaneStack {
    /// The objects of a resumed frame's chain, now the activation at `depth`'s, moved off its
    /// record.
    pub(super) fn restore_with_objects(&mut self, depth: usize, held: &mut Vec<Value>) {
        self.with_objects.extend(held.drain(..).map(|object| (depth, object)));
    }

    /// The chain of the activation at `depth`, the innermost, taken off the stack: what a
    /// suspended frame keeps (`FrameRecord::suspend_at`).
    pub(in crate::vm) fn drain_with_objects(&mut self, depth: usize) -> WithChain {
        let first = self.with_objects.partition_point(|(owner, _)| *owner < depth);
        WithChain::from_iter(self.with_objects.drain(first..).map(|(_, object)| object))
    }

    /// The chain of the activation at `depth`, outermost first.
    pub(in crate::vm) fn with_chain(&self, depth: usize) -> impl Iterator<Item = Value> + '_ {
        self.with_objects.iter().filter(move |(owner, _)| *owner == depth).map(|(_, object)| *object)
    }

    /// Leaves the `with` scopes of the activation at `depth`, the innermost, past the first
    /// `entered` of its chain: what a handler's entry unwinds (`Vm::unwind_handler_scopes`).
    pub(in crate::vm) fn unwind_with_objects(&mut self, depth: usize, entered: usize) {
        let first = self.with_objects.partition_point(|(owner, _)| *owner < depth);
        self.with_objects.truncate(first + entered);
    }

    /// Drops the objects of the activations from index `depth` on, which returned from inside a
    /// `with` statement: the compiler emits no scope exit before a `return`.
    #[inline(always)]
    pub(super) fn drop_with_objects_from(&mut self, depth: usize) {
        if !self.with_objects.is_empty() {
            self.drop_with_objects_held_from(depth);
        }
    }

    #[cold]
    #[inline(never)]
    fn drop_with_objects_held_from(&mut self, depth: usize) {
        self.with_objects.retain(|(owner, _)| *owner < depth);
    }

    /// Every object a chain holds, which the collector keeps alive.
    pub(in crate::vm) fn with_object_roots(&self) -> impl Iterator<Item = Value> + '_ {
        self.with_objects.iter().map(|(_, object)| *object)
    }
}

/// `with (object)` (`Native::WithEnter`) or the exit of the innermost `with` (`Native::WithExit`)
/// in the innermost lane activation, which owns the scope: the object joins, or leaves, its
/// chain. `None`, before any effect, leaves the call to the native, which a frame's run needs
/// (its scope is the VM's stack) and which boxes a primitive or throws for a nullish operand.
pub(super) fn native_with_scope<H: Host>(cx: *mut LaneContext<H>, native: Native, args: &[Value]) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if vm.lane_deferred.is_some() || vm.lane_stack.native_boundary.is_some() {
        return None;
    }
    let depth = vm.lane_stack.activations.len().checked_sub(1)?;
    vm.lane_stack.activations.last().and_then(Activation::script)?;
    if native == Native::WithEnter {
        let object = args.first().copied().filter(|value| vm.is_object_like(*value))?;
        vm.lane_stack.with_objects.push((depth, object));
        vm.with_scope_entered = true;
    } else {
        vm.lane_stack.with_objects.pop_if(|(owner, _)| *owner == depth)?;
    }
    Some(Value::UNDEFINED)
}
