//! Where an activation's variable environment lives: the `var`s a direct eval introduces and
//! the binding sites its closures capture are state of one owner, a VM frame or the record of
//! a lane activation (`FrameRecord`), and every reader and mutator of that state goes through
//! the owner, not through a frame index.
use super::activation::FrameRecord;
use super::caller_view::CallerView;
use super::*;

/// The owner of a variable environment. A record's pointer is its activation's own, which the
/// activation's slot or its generator or async owner keeps until the activation ends.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::vm) enum VarOwner {
    Frame(usize),
    Record(*mut FrameRecord),
}

/// A direct eval run from a lane activation: the activation's record, which owns the eval's
/// variable environment, and the depth of the frame stack the eval's code runs above
/// (`Vm::eval_callers`).
#[derive(Clone, Copy)]
pub(in crate::vm) struct EvalCaller {
    pub(in crate::vm) frames: usize,
    pub(in crate::vm) record: *mut FrameRecord,
}

impl<H: Host> Vm<H> {
    /// The view of the activation `owner` names.
    pub(super) fn owner_view(&self, owner: VarOwner) -> &dyn CallerView {
        match owner {
            VarOwner::Frame(index) => self.frame_caller(index),
            // SAFETY: a record's owner keeps it, unmoved, while an activation or eval runs over it.
            VarOwner::Record(record) => unsafe { &*record },
        }
    }

    /// The locals and the held dynamic bindings of `owner`'s activation.
    fn owner_state_mut(&mut self, owner: VarOwner) -> (&mut Vec<Value>, &mut Vec<(Atom, Value)>) {
        match owner {
            VarOwner::Frame(index) => {
                let frame = &mut *self.frames[index];
                (&mut frame.locals, &mut frame.dynamic_bindings)
            }
            // SAFETY: as `owner_view`; nothing else accesses the record during a mutation.
            VarOwner::Record(record) => unsafe { (&mut (*record).locals, &mut (*record).dynamic_bindings) },
        }
    }

    /// The dynamic bindings `owner` holds: its environment's once that holds them, else its own.
    pub(super) fn owner_bindings_mut(&mut self, owner: VarOwner) -> Option<&mut Vec<(Atom, Value)>> {
        let view = self.owner_view(owner);
        if view.owns_environment() {
            let environment = view.environment();
            self.heap.environment_bindings_mut(environment)
        } else {
            Some(self.owner_state_mut(owner).1)
        }
    }

    /// `value` stored to the dynamic binding `atom` `owner` holds; `false` where it holds none.
    pub(super) fn store_owner_binding(&mut self, owner: VarOwner, atom: Atom, value: Value) -> bool {
        let Some((_, binding)) = self
            .owner_bindings_mut(owner)
            .and_then(|bindings| bindings.iter_mut().rev().find(|(candidate, _)| *candidate == atom))
        else {
            return false;
        };
        *binding = value;
        true
    }

    /// Whether `slot` of `owner`'s function lives in its environment, which it has made.
    fn owner_slot_in_environment(&self, owner: VarOwner, slot: usize) -> bool {
        let view = self.owner_view(owner);
        view.owns_environment()
            && self.programs.get(view.program()).is_none_or(|program| {
                program
                    .function_get(view.function() as usize)
                    .is_none_or(|function| function.local_slot_uses_environment(slot))
            })
    }

    /// The value of the binding `atom` of `owner`'s function that lives in its slots.
    pub(super) fn owner_binding_value(&self, owner: VarOwner, atom: Atom) -> Option<Value> {
        let view = self.owner_view(owner);
        let slot = self.activation_binding_slot(view, atom)?;
        if self.owner_slot_in_environment(owner, slot) {
            return self.heap.environment_slot(view.environment(), slot);
        }
        self.owner_local(owner, slot)
    }

    /// The local `slot` of `owner`'s activation, whose own it is while its environment holds
    /// none of them.
    pub(super) fn owner_local(&self, owner: VarOwner, slot: usize) -> Option<Value> {
        match owner {
            VarOwner::Frame(index) => self.frames[index].locals.get(slot).copied(),
            // SAFETY: as `owner_view`.
            VarOwner::Record(record) => unsafe { &*record }.locals.get(slot).copied(),
        }
    }

    /// `value` stored to `slot` of `owner`'s function, mirrored into a mapped `arguments` of a
    /// frame (a record's activation reflects none: `Function::locals_ride_window`); `false`
    /// where it has no such slot.
    pub(super) fn store_owner_slot(&mut self, owner: VarOwner, slot: usize, value: Value) -> bool {
        let environment = self.owner_slot_in_environment(owner, slot).then(|| self.owner_view(owner).environment());
        let binding = match environment {
            Some(environment) => self.heap.environment_slot_mut(environment, slot),
            None => self.owner_state_mut(owner).0.get_mut(slot),
        };
        let Some(binding) = binding else {
            return false;
        };
        *binding = value;
        if let VarOwner::Frame(frame) = owner
            && let Some(program) = self.programs.get(self.frames[frame].program)
        {
            self.mapped_argument_store(&program, frame, slot, value);
        }
        true
    }

    /// The environment `depth` function layers out from `owner`'s captures.
    pub(super) fn owner_capture_env(&self, owner: VarOwner, depth: u16) -> Option<Value> {
        self.caller_capture_env(self.owner_view(owner), depth)
    }

    /// The activation a direct eval runs for, innermost: the record of the lane activation that
    /// called it where no frame was pushed since (`eval_callers`), else the innermost frame.
    pub(super) fn eval_owner(&self) -> Option<VarOwner> {
        match self.eval_callers.last() {
            Some(caller) if caller.frames == self.frames.len() => Some(VarOwner::Record(caller.record)),
            _ => self.frames.len().checked_sub(1).map(VarOwner::Frame),
        }
    }

    /// The record the eval whose root runs in the frame at `frame` was called from, if a lane
    /// activation called it.
    pub(super) fn eval_record_at(&self, frame: usize) -> Option<*mut FrameRecord> {
        self.eval_callers.iter().rev().find(|caller| caller.frames == frame).map(|caller| caller.record)
    }

    /// The owner of the variable environment the activation `activation` runs in: itself, or,
    /// for the root of a sloppy direct eval in its caller's scope, its caller's.
    pub(super) fn variable_environment_owner(&self, activation: VarOwner) -> VarOwner {
        let VarOwner::Frame(mut frame) = activation else {
            return activation;
        };
        while self.frames[frame].function == super::ROOT_FUNCTION_ID && self.programs.runs_in_caller_scope(self.frames[frame].program) {
            if let Some(record) = self.eval_record_at(frame) {
                return VarOwner::Record(record);
            }
            let Some(caller) = frame.checked_sub(1) else {
                break;
            };
            frame = caller;
        }
        VarOwner::Frame(frame)
    }

    /// The environment of `owner`'s activation, made if it has none: a frame promotes its locals
    /// (`promote_frame_environment`); a record's was promoted when the lane entered the eval
    /// (`lane_eval::promote_window`), so it holds one.
    pub(super) fn promote_owner_environment(&mut self, owner: VarOwner) -> Value {
        match owner {
            VarOwner::Frame(frame) => self.promote_frame_environment(frame),
            VarOwner::Record(_) => self.owner_view(owner).environment(),
        }
    }
}
