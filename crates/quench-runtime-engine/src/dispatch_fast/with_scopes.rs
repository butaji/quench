//! The `with` scope chain of a lane activation: data on the lane stack, as a frame's is on the
//! VM's scope stack. An activation's chain is its closure's captured objects then the ones it
//! entered, outermost first, each tagged with the activation that owns it
//! (`LaneStack::with_objects`); a suspended frame's record keeps it while no activation runs
//! the frame (`FrameRecord::with_objects`), and a materialized activation's frame takes it.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

pub(crate) use quench_runtime::vm::engine_types::WithChain;

#[quench_macros::ext(LaneStackOps7)]
impl LaneStack {
    /// The objects of a resumed frame's chain, now the activation at `depth`'s, moved off its
    /// record.
    #[inline(always)]
    pub(crate) fn restore_with_objects(&mut self, depth: usize, record: &mut quench_runtime::vm::activation::FrameRecord) {
        if let Some(held) = record.held_with_objects_mut()
            && !held.is_empty()
        {
            self.with_objects.extend(held.drain(..).map(|object| (depth, object)));
        }
    }

    /// The chain of the activation at `depth`, outermost first.
    pub(crate) fn with_chain(&self, depth: usize) -> impl Iterator<Item = Value> + '_ {
        self.with_objects.iter().filter(move |(owner, _)| *owner == depth).map(|(_, object)| *object)
    }

    /// Drops the objects of the activations from index `depth` on, which returned from inside a
    /// `with` statement: the compiler emits no scope exit before a `return`.
    #[inline(always)]
    pub(crate) fn drop_with_objects_from(&mut self, depth: usize) {
        if !self.with_objects.is_empty() {
            self.drop_with_objects_held_from(depth);
        }
    }

    #[cold]
    #[inline(never)]
    fn drop_with_objects_held_from(&mut self, depth: usize) {
        self.with_objects.retain(|(owner, _)| *owner < depth);
    }
}

#[quench_macros::ext(DispatchFastWithScopesVm)]
impl Vm {
    /// A `with` statement entered, in a frame or a lane activation: names resolve through object
    /// environments from now on (`Vm::with_scope_entered`). Calls activate in the lane after it
    /// as before: no lane target's function inherits a `with` scope (`CallPlan`), and a function
    /// of eval code, whose frame takes the chain its environment captured
    /// (`Vm::captured_with_objects_for_function`), resolves to no target (`Vm::user_target_in_lane`).
    /// The first entry retires every target resolved before, on the running lane stack, the
    /// parked ones and the spares.
    pub(crate) fn enter_with_scope(&mut self) {
        if self.with_scope_entered {
            return;
        }
        self.with_scope_entered = true;
        self.lane_stack.advance_generation();
        self.lane_roots.iter_mut().for_each(|parked| parked.stack.advance_generation());
        self.spare_lane_stacks.iter_mut().for_each(|spare| spare.advance_generation());
    }
}

/// `with (object)` (`Native::WithEnter`) or the exit of the innermost `with` (`Native::WithExit`)
/// in the innermost lane activation, which owns the scope: the object joins, or leaves, its
/// chain. `None`, before any effect, leaves the call to the native, which a frame's run needs
/// (its scope is the VM's stack) and which boxes a primitive or throws for a nullish operand.
pub(crate) fn native_with_scope(cx: *mut LaneContext, native: Native, args: &[Value]) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if vm.lane_deferred.is_some() || vm.lane_stack.native_boundary.is_some() {
        return None;
    }
    let depth = vm.lane_stack.activation_count().checked_sub(1)?;
    vm.lane_stack.top_frame().and_then(|frame| frame.script())?;
    if native == Native::WithEnter {
        let object = args.first().copied().filter(|value| vm.is_object_like(*value))?;
        vm.lane_stack.with_objects.push((depth, object));
        vm.enter_with_scope();
    } else {
        vm.lane_stack.with_objects.pop_if(|(owner, _)| *owner == depth)?;
    }
    Some(Value::UNDEFINED)
}
