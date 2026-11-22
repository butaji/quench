//! The window activation as the owner of a variable environment (`VarOwner::Window`): what a
//! name lookup, a local or a capture step reads of the activation a lane runs without a VM
//! frame, through the same accessors a frame's owner offers (`var_owner`). The activation's
//! environment window (`WINDOW_ENVIRONMENT`, `WINDOW_LOCALS`) holds its environment and its
//! locals, its registers the operands, its record (`FrameRecord`) the dynamic bindings a direct
//! eval made, and the lane stack its `with` objects (`LaneStack::with_chain`).
use super::*;
use super::with_scopes::WithChain;
use quench_runtime::vm::activation::FrameRecord;
use quench_runtime::vm::caller_view::CallerView;
use quench_runtime::vm::var_owner::VarOwner;
use quench_runtime::bytecode::Atom;

/// What a lookup reads of the window activation making it, which it reads as it reads a frame
/// (`CallerView`), and the registers and locals its steps read and write.
pub(crate) struct WindowScope {
    pub(super) function: u32,
    pub(super) program: ProgramId,
    /// The activation's receiver as the lane holds it.
    pub(super) this: Value,
    /// The activation's environment: its own once a closure made one (`captured`), else the
    /// closure's.
    pub(super) env: Value,
    pub(super) captured: bool,
    /// The activation's record, null for an ordinary function's: it holds no dynamic bindings,
    /// which only a frame's direct eval makes.
    pub(super) record: *mut FrameRecord,
    /// The resume pc of the binding site the access is at (`Frame::binding_site_pc`), if it is
    /// at one.
    pub(super) site: Option<u32>,
    /// The `with` objects of the activation's scope chain (`LaneStack::with_chain`), outermost
    /// first, which no `with` statement has entered while none is (`Vm::with_scope_entered`).
    pub(super) chain: WithChain,
    /// The activation's register file, which its environment window follows.
    pub(super) registers: Registers,
    /// The first of the window's locals, which the activation's `Function::locals` fill.
    pub(super) locals: *mut Value,
    pub(super) local_count: usize,
}

impl WindowScope {
    /// The dynamic bindings the activation's record holds, until a closure's environment takes
    /// them over (`captured`); none for an activation without a record.
    pub(crate) fn record_bindings(&self) -> Option<&mut Vec<(Atom, Value)>> {
        // SAFETY: the record is the running activation's own, which nothing else accesses.
        (!self.captured && !self.record.is_null()).then(|| unsafe { (*self.record).dynamic_bindings_mut() })
    }

    /// The window's locals.
    pub(crate) fn locals(&self) -> &[Value] {
        // SAFETY: the window holds `Function::locals` locals after its entries, which nothing
        // else accesses while a step of the activation runs.
        unsafe { std::slice::from_raw_parts(self.locals, self.local_count) }
    }

    /// The window's locals, to write.
    pub(crate) fn locals_mut(&mut self) -> &mut [Value] {
        // SAFETY: as `locals`.
        unsafe { std::slice::from_raw_parts_mut(self.locals, self.local_count) }
    }

    /// The register file's `register`.
    pub(crate) fn register(&self, register: u16) -> Value {
        self.registers.get(register)
    }

    pub(crate) fn set_register(&self, register: u16, value: Value) {
        self.registers.set(register, value);
    }

    /// Replaces the binding site the activation's accesses are at, which `site` names.
    pub(crate) fn replace_site(&mut self, site: Option<u32>) -> Option<u32> {
        std::mem::replace(&mut self.site, site)
    }
}

impl CallerView for WindowScope {
    fn program(&self) -> ProgramId {
        self.program
    }

    fn function(&self) -> u32 {
        self.function
    }

    fn this(&self) -> Value {
        self.this
    }

    fn environment(&self) -> Value {
        self.env
    }

    fn owns_environment(&self) -> bool {
        self.captured
    }

    fn binding_site(&self) -> Option<u32> {
        self.site
    }

    fn held_bindings(&self) -> &[(Atom, Value)] {
        self.record_bindings().map_or(&[], |bindings| bindings.as_slice())
    }

    fn scope_chain<'a>(&'a self, _entered: &'a [Value]) -> &'a [Value] {
        &self.chain
    }
}

#[quench_macros::ext(DispatchFastWindowOwnerVm)]
impl Vm {
    /// The owner of the variable environment of the activation that runs: the innermost frame,
    /// or, beside the lane's frameless activations (`lane_deferred`), the innermost activation's
    /// window, which a step of its function reads where it stands (a plain activation's, registers only,
    /// has no locals: `scoped_names::plain_scope`).
    pub(crate) fn running_owner(&mut self) -> VarOwner {
        if let Some(DeferredLaneCall { cx, call_pc, .. }) = self.lane_deferred {
            let cx = cx.cast::<LaneContext>();
            // SAFETY: the deferred call's lane runs, so its context lives, and the call's record
            // lies in the view of the innermost activation's function.
            let (registers, ip) = unsafe { ((*cx).registers, (*cx).base.add(call_pc)) };
            self.running_window = super::scoped_names::window_scope(cx, registers, ip, false).or_else(|| super::scoped_names::plain_scope(cx, registers));
            if self.running_window.is_some() {
                return VarOwner::Window;
            }
            self.materialize_deferred_lane();
        }
        VarOwner::Frame(self.frames.len() - 1)
    }
}
