//! The window activation as the owner of a variable environment (`VarOwner::Window`): what a
//! name lookup, a local or a capture step reads of the activation a lane runs without a VM
//! frame, through the same accessors a frame's owner offers (`var_owner`). The activation's
//! environment window (`WINDOW_ENVIRONMENT`, `WINDOW_LOCALS`) holds its environment and its
//! locals, its registers the operands, its record (`FrameRecord`) the dynamic bindings a direct
//! eval made, and the lane stack its `with` objects (`LaneStack::with_chain`).
use super::*;
use super::super::activation::FrameRecord;
use super::super::caller_view::CallerView;
use super::super::var_owner::VarOwner;
use crate::bytecode::Atom;

/// What a lookup reads of the window activation making it, which it reads as it reads a frame
/// (`CallerView`), and the registers and locals its steps read and write.
pub(in crate::vm) struct WindowScope {
    pub(super) function: u32,
    pub(super) program: ProgramId,
    /// The activation's receiver as the lane holds it.
    this: Value,
    /// The activation's environment: its own once a closure made one (`captured`), else the
    /// closure's.
    env: Value,
    captured: bool,
    /// The activation's record, null for an ordinary function's: it holds no dynamic bindings,
    /// which only a frame's direct eval makes.
    pub(super) record: *mut FrameRecord,
    /// The resume pc of the binding site the access is at (`Frame::binding_site_pc`), if it is
    /// at one.
    pub(super) site: Option<u32>,
    /// The `with` objects of the activation's scope chain (`LaneStack::with_chain`), outermost
    /// first, which no `with` statement has entered while none is (`Vm::with_scope_entered`).
    pub(super) chain: Vec<Value>,
    /// The activation's register file, which its environment window follows.
    registers: Registers,
    /// The first of the window's locals, which the activation's `Function::locals` fill.
    locals: *mut Value,
    local_count: usize,
}

/// The scope of the innermost activation, when it is an environment window: registers `r` and
/// the record at `ip` of its function are the access's. `at_site` where the access is an
/// instruction a binding site scopes (`Frame::binding_site_pc`). `None` in a frame's run.
pub(super) fn window_scope<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, at_site: bool) -> Option<WindowScope> {
    // SAFETY: the context's VM is the one running the lane.
    window_scope_in(unsafe { vm(cx) }, cx, r, ip, at_site)
}

/// `window_scope` of the activations of `vm`.
fn window_scope_in<H: Host>(vm: &Vm<H>, cx: *mut LaneContext<H>, r: Registers, ip: Ip, at_site: bool) -> Option<WindowScope> {
    let depth = vm.lane_stack.activations.len().checked_sub(1)?;
    let activation = vm.lane_stack.activations.last()?;
    let script = activation.script().filter(|script| script.environment)?;
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the running
    // function's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    let body = code.function(activation.function as usize);
    // SAFETY: the activation's window follows its registers (`lane_activate_environment`).
    let entries = unsafe { r.0.add(usize::from(body.registers)) };
    let own = r.get(body.registers + WINDOW_ENVIRONMENT);
    let captured = !own.is_deleted();
    Some(WindowScope {
        function: activation.function,
        program: unsafe { (*cx).program },
        this: unsafe { (*cx).this },
        env: if captured { own } else { unsafe { (*cx).env } },
        captured,
        record: script.record,
        site: at_site.then_some(pc as u32 + 1),
        chain: if vm.with_scope_entered { vm.lane_stack.with_chain(depth).collect() } else { Vec::new() },
        registers: r,
        locals: unsafe { entries.add(usize::from(WINDOW_LOCALS)) },
        local_count: usize::from(body.locals),
    })
}

impl WindowScope {
    /// The dynamic bindings the activation's record holds, until a closure's environment takes
    /// them over (`captured`); none for an activation without a record.
    pub(super) fn record_bindings(&self) -> Option<&mut Vec<(Atom, Value)>> {
        // SAFETY: the record is the running activation's own, which nothing else accesses.
        (!self.captured && !self.record.is_null()).then(|| unsafe { &mut (*self.record).dynamic_bindings })
    }

    /// The window's locals.
    pub(in crate::vm) fn locals(&self) -> &[Value] {
        // SAFETY: the window holds `Function::locals` locals after its entries, which nothing
        // else accesses while a step of the activation runs.
        unsafe { std::slice::from_raw_parts(self.locals, self.local_count) }
    }

    /// The window's locals, to write.
    pub(in crate::vm) fn locals_mut(&mut self) -> &mut [Value] {
        // SAFETY: as `locals`.
        unsafe { std::slice::from_raw_parts_mut(self.locals, self.local_count) }
    }

    /// The register file's `register`.
    pub(in crate::vm) fn register(&self, register: u16) -> Value {
        self.registers.get(register)
    }

    pub(in crate::vm) fn set_register(&self, register: u16, value: Value) {
        self.registers.set(register, value);
    }

    /// Replaces the binding site the activation's accesses are at, which `site` names.
    pub(in crate::vm) fn replace_site(&mut self, site: Option<u32>) -> Option<u32> {
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

impl<H: Host> Vm<H> {
    /// The owner of the variable environment of the activation that runs: the innermost frame,
    /// or, beside the lane's frameless activations (`lane_deferred`), the innermost activation's
    /// window, which a step of its function reads where it stands. A function whose activations
    /// hold no window (`ScriptActivation::environment`) has the activations made frames first, as
    /// guest code would make them.
    pub(in crate::vm) fn running_owner(&mut self) -> VarOwner {
        if let Some(DeferredLaneCall { cx, call_pc, .. }) = self.lane_deferred {
            let cx = cx.cast::<LaneContext<H>>();
            // SAFETY: the deferred call's lane runs, so its context lives, and the call's record
            // lies in the view of the innermost activation's function.
            let (registers, ip) = unsafe { ((*cx).registers, (*cx).base.add(call_pc)) };
            self.running_window = window_scope_in(self, cx, registers, ip, false);
            if self.running_window.is_some() {
                return VarOwner::Window;
            }
            self.materialize_deferred_lane();
        }
        VarOwner::Frame(self.frames.len() - 1)
    }
}
