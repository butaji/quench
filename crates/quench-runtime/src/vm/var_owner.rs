//! Where an activation's variable environment lives: the `var`s a direct eval introduces and
//! the binding sites its closures capture are state of one owner, a VM frame, the record of a
//! lane activation (`FrameRecord`) or the window of the activation a lane runs without a frame,
//! and every reader and mutator of that state goes through the owner, not through a frame index.
use super::activation::FrameRecord;
use super::caller_view::CallerView;
use super::*;

/// The owner of a variable environment. A record's pointer is its activation's own, which the
/// activation's slot or its generator or async owner keeps until the activation ends.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VarOwner {
    Frame(usize),
    Record(*mut FrameRecord),
    /// The innermost activation of a lane native call's frameless activations, whose scope
    /// `Vm::running_owner` read (`Vm::running_window`): its environment window holds its locals
    /// and its registers its operands. Once guest code made the activations frames
    /// (`Vm::materialize_deferred_lane`) it is the innermost frame.
    Window,
}

/// A direct eval run from a lane activation: the activation's record, which owns the eval's
/// variable environment, and the depth of the frame stack the eval's code runs above
/// (`Vm::eval_callers`).
#[derive(Clone, Copy)]
pub struct EvalCaller {
    pub frames: usize,
    pub record: *mut FrameRecord,
}

impl Vm {
    /// `owner` as it stands: a window owner whose activations became frames is the innermost frame.
    fn live_owner(&self, owner: VarOwner) -> VarOwner {
        match owner {
            VarOwner::Window if self.lane_deferred.is_none() => VarOwner::Frame(self.frames.len() - 1),
            _ => owner,
        }
    }

    fn window(&self) -> &crate::vm::engine_types::WindowScope {
        self.running_window.as_ref().expect("a window owner names the scope running_owner read")
    }

    fn window_mut(&mut self) -> &mut crate::vm::engine_types::WindowScope {
        self.running_window.as_mut().expect("a window owner names the scope running_owner read")
    }

    /// The view of the activation `owner` names.
    pub(super) fn owner_view(&self, owner: VarOwner) -> &dyn CallerView {
        match self.live_owner(owner) {
            VarOwner::Frame(index) => self.frame_caller(index),
            // SAFETY: a record's owner keeps it, unmoved, while an activation or eval runs over it.
            VarOwner::Record(record) => unsafe { &*record },
            VarOwner::Window => self.window(),
        }
    }

    /// The frame `owner` names, for the state only a frame has: its mapped `arguments`, its
    /// capture cache, its eval root. A record's activation and a window reflect none.
    pub(super) fn owner_frame(&self, owner: VarOwner) -> Option<usize> {
        match self.live_owner(owner) {
            VarOwner::Frame(index) => Some(index),
            VarOwner::Record(_) | VarOwner::Window => None,
        }
    }

    /// The bound `this` of `owner`'s frame (`checked_this_binding`: an unbound one, a TDZ read, raises); `None`
    /// where `owner` names no frame. The one body of the `this` row both VM frames run (`HostFrame`,
    /// `InterpreterFrame`).
    #[inline(always)]
    pub(super) fn owner_this(&mut self, program: &ResidualProgram, owner: VarOwner) -> Option<Result<Value, JsError>> {
        let frame = self.owner_frame(owner)?;
        Some(self.checked_this_binding(program, frame))
    }

    /// The callee of `owner`'s frame (`undefined` for one with none); `None` where `owner` names no frame.
    #[inline(always)]
    pub(super) fn owner_callee(&self, owner: VarOwner) -> Option<Value> {
        let frame = self.owner_frame(owner)?;
        Some(self.frames[frame].context.callee().unwrap_or(Value::UNDEFINED))
    }

    /// The locals of `owner`'s activation.
    pub(super) fn owner_locals(&self, owner: VarOwner) -> &[Value] {
        match self.live_owner(owner) {
            VarOwner::Frame(index) => &self.frames[index].locals,
            // SAFETY: as `owner_view`.
            VarOwner::Record(record) => unsafe { &(*record).locals },
            VarOwner::Window => self.window().locals(),
        }
    }

    fn owner_locals_mut(&mut self, owner: VarOwner) -> &mut [Value] {
        match self.live_owner(owner) {
            VarOwner::Frame(index) => &mut self.frames[index].locals,
            // SAFETY: as `owner_view`; nothing else accesses the record during a mutation.
            VarOwner::Record(record) => unsafe { &mut (*record).locals },
            VarOwner::Window => self.window_mut().locals_mut(),
        }
    }

    /// The dynamic bindings `owner`'s activation holds itself: none for a window without a record.
    fn owner_held_bindings_mut(&mut self, owner: VarOwner) -> Option<&mut Vec<(Atom, Value)>> {
        match self.live_owner(owner) {
            VarOwner::Frame(index) => Some(&mut self.frames[index].dynamic_bindings),
            // SAFETY: as `owner_locals_mut`.
            VarOwner::Record(record) => Some(unsafe { (*record).dynamic_bindings_mut() }),
            VarOwner::Window => self.window().record_bindings(),
        }
    }

    /// The dynamic bindings `owner` holds: its environment's once that holds them, else its own.
    pub(super) fn owner_bindings_mut(&mut self, owner: VarOwner) -> Option<&mut Vec<(Atom, Value)>> {
        let view = self.owner_view(owner);
        if view.owns_environment() {
            let environment = view.environment();
            self.heap.environment_bindings_mut(environment)
        } else {
            self.owner_held_bindings_mut(owner)
        }
    }

    /// `register` of `owner`'s register file: the interpreter frame's, or the window's.
    pub(super) fn owner_register(&self, owner: VarOwner, register: u16) -> Value {
        match self.live_owner(owner) {
            VarOwner::Frame(index) => self.read(index, register),
            VarOwner::Window => self.window().register(register),
            VarOwner::Record(_) => unreachable!("a record's registers are not read while its activation is suspended"),
        }
    }

    pub(super) fn set_owner_register(&mut self, owner: VarOwner, register: u16, value: Value) {
        match self.live_owner(owner) {
            VarOwner::Frame(index) => self.write(index, register, value),
            VarOwner::Window => self.window().set_register(register, value),
            VarOwner::Record(_) => unreachable!("a record's registers are not written while its activation is suspended"),
        }
    }

    /// Makes `site` the binding site `owner`'s accesses are at, which `previous` it returns restores.
    pub(super) fn replace_binding_site(&mut self, owner: VarOwner, site: Option<u32>) -> Option<u32> {
        match self.live_owner(owner) {
            VarOwner::Frame(index) => std::mem::replace(&mut self.frames[index].binding_site_pc, site),
            // SAFETY: as `owner_locals_mut`.
            VarOwner::Record(record) => unsafe { (*record).replace_binding_site_pc(site) },
            VarOwner::Window => self.window_mut().replace_site(site),
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
    pub(super) fn slot_is_environment_owned(&self, p: &ResidualProgram, owner: VarOwner, slot: usize) -> bool {
        let view = self.owner_view(owner);
        view.owns_environment()
            && p.function_get(view.function() as usize).is_none_or(|function| function.local_slot_uses_environment(slot))
    }

    /// The binding `owner`'s activation holds for `slot`: its environment's slot once the slot
    /// lives there, else its local. `None` for a slot outside both.
    pub(super) fn owner_binding(&self, p: &ResidualProgram, owner: VarOwner, slot: usize) -> Option<Value> {
        if self.slot_is_environment_owned(p, owner, slot) {
            self.heap.environment_slot(self.owner_view(owner).environment(), slot)
        } else {
            self.owner_locals(owner).get(slot).copied()
        }
    }

    /// `owner_binding`, to write.
    pub(super) fn owner_binding_mut(&mut self, p: &ResidualProgram, owner: VarOwner, slot: usize) -> Option<&mut Value> {
        if self.slot_is_environment_owned(p, owner, slot) {
            let environment = self.owner_view(owner).environment();
            self.heap.environment_slot_mut(environment, slot)
        } else {
            self.owner_locals_mut(owner).get_mut(slot)
        }
    }

    /// The value of the binding `atom` of `owner`'s function that lives in its slots.
    pub(super) fn owner_binding_value(&self, owner: VarOwner, atom: Atom) -> Option<Value> {
        let view = self.owner_view(owner);
        let slot = self.activation_binding_slot(view, atom)?;
        let program = self.programs.get(view.program())?;
        self.owner_binding(&program, owner, slot)
    }

    /// The local `slot` of `owner`'s activation, whose own it is while its environment holds
    /// none of them.
    pub(super) fn owner_local(&self, owner: VarOwner, slot: usize) -> Option<Value> {
        self.owner_locals(owner).get(slot).copied()
    }

    /// `value` stored to `slot` of `owner`'s function, mirrored into a mapped `arguments`;
    /// `false` where it has no such slot.
    pub(super) fn store_owner_slot(&mut self, owner: VarOwner, slot: usize, value: Value) -> bool {
        let Some(program) = self.programs.get(self.owner_view(owner).program()) else {
            return false;
        };
        let Some(binding) = self.owner_binding_mut(&program, owner, slot) else {
            return false;
        };
        *binding = value;
        self.mapped_argument_store(&program, owner, slot, value);
        true
    }

    /// The environment `depth` function layers out from `owner`'s captures.
    pub fn owner_capture_env(&self, owner: VarOwner, depth: u16) -> Option<Value> {
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
            VarOwner::Window => unreachable!("a window's environment is made by its closures (`window_environment`)"),
        }
    }
}
