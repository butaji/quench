//! Names a window activation resolves through its scope: the `with` objects of its chain
//! (`LaneStack::with_chain`) and the dynamic bindings of its record (`FrameRecord`), which only a
//! suspended function has. A function that reaches names this way (`CallPlan::holds_window_scope`) runs
//! its by-name loads and stores, local stores and closures here instead of in a frame, where
//! `Vm::find_with_binding`, `Vm::load_name_call` and `Vm::closure_in_realm` make them. A lookup
//! the shapes of the objects alone do not decide, or that no `with` object, dynamic binding or
//! global read past every scope (`Vm::read_past_scopes`) answers, leaves the instruction to the
//! general path before any effect.
use super::*;
use super::window_owner::WindowScope;
use super::with_scopes::WithChain;
use quench_runtime::bytecode::Atom;

/// The scope of the innermost activation, when it is an environment window: registers `r` and
/// the record at `ip` of its function are the access's. `at_site` where the access is an
/// instruction a binding site scopes (`Frame::binding_site_pc`). `None` in a frame's run.
pub(crate) fn window_scope(cx: *mut LaneContext, r: Registers, ip: Ip, at_site: bool) -> Option<WindowScope> {
    let vm = unsafe { vm(cx) };
    let depth = vm.lane_stack.activation_count().checked_sub(1)?;
    let activation = vm.lane_stack.top_frame()?;
    let script = activation.script().filter(|script| script.environment)?;
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the running
    // function's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    let body = code.function(activation.function() as usize);
    // SAFETY: the activation's window follows its registers (`activate_environment_in_lane`).
    let entries = unsafe { r.0.add(usize::from(body.registers)) };
    let own = r.get(body.registers + WINDOW_ENVIRONMENT);
    let captured = !own.is_deleted();
    Some(WindowScope {
        function: activation.function(),
        program: unsafe { (*cx).program },
        this: unsafe { (*cx).registers }.this(),
        env: if captured { own } else { unsafe { (*cx).registers }.env() },
        captured,
        record: script.record,
        site: at_site.then_some(pc as u32 + 1),
        chain: if vm.with_scope_entered { WithChain::from_iter(vm.lane_stack.with_chain(depth)) } else { WithChain::new() },
        registers: r,
        locals: unsafe { entries.add(usize::from(WINDOW_LOCALS)) },
        local_count: usize::from(body.locals),
    })
}

/// The innermost activation as the owner of its steps where it is a plain one (registers only, no locals window): its
/// closure's environment is the one its captures read, it has no locals, no record and no `with` objects. `None` in an
/// environment window (`window_scope`) or a frame's run.
pub(crate) fn plain_scope(cx: *mut LaneContext, r: Registers) -> Option<WindowScope> {
    let vm = unsafe { vm(cx) };
    let activation = vm.lane_stack.top_frame()?;
    let script = activation.script().filter(|script| !script.environment)?;
    Some(WindowScope {
        function: activation.function(),
        program: unsafe { (*cx).program },
        this: unsafe { (*cx).registers }.this(),
        env: unsafe { (*cx).registers }.env(),
        captured: false,
        record: script.record,
        site: None,
        chain: WithChain::new(),
        registers: r,
        locals: r.0,
        local_count: 0,
    })
}

#[quench_macros::ext(DispatchFastScopedNamesVm)]
impl Vm {
    /// The innermost object of `scope`'s chain that has `atom`, with its value there, as
    /// `find_with_binding` finds it for a frame: `Some(None)` where none does, `None` where the
    /// objects' shapes alone do not decide. The lookup reads the scope as a frame's and runs
    /// no guest code.
    fn window_with_binding(&self, scope: &WindowScope, atom: Atom) -> Option<Option<(Value, Value)>> {
        if scope.chain.is_empty() || self.atom_class(atom).contains(AtomClass::INTERNAL) {
            return Some(None);
        }
        let objects = self.with_objects_before_binding(scope, atom);
        self.plain_first_with_binding(&objects, atom)
    }
}

/// The read of `atom` the window activation at `ip` makes by name, as `Vm::load_name_call`
/// makes it: its value and its call's receiver, the `with` object that has it, else the dynamic
/// binding it resolves (`Vm::dynamic_binding`): its own, which a direct eval declared in its record's
/// environment, or an enclosing activation's; else, where no scope binds it, the global read
/// (`Vm::read_past_scopes`, its cached field at `site`), as a direct eval's `eval` or a global's
/// name is read. `None`, before any effect, leaves it to the general path.
fn scoped_name_read(cx: *mut LaneContext, r: Registers, ip: Ip, atom: Atom, site: u16) -> Option<(Value, Value)> {
    let vm = unsafe { vm(cx) };
    if vm.atom_class(atom).contains(AtomClass::INTERNAL) {
        return None;
    }
    let scope = window_scope(cx, r, ip, true)?;
    if let Some((object, value)) = vm.window_with_binding(&scope, atom)? {
        return Some((value, object));
    }
    if let Some(value) = vm.dynamic_binding(&scope, atom) {
        return (!value.is_deleted()).then_some((value, Value::UNDEFINED));
    }
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    vm.read_past_scopes(code, &scope, atom, site).map(|value| (value, Value::UNDEFINED))
}

/// How a store by name of a window activation went among its `with` objects.
enum ScopedStore {
    /// The object that has the name took it, as `Vm::store_with_binding` stores to it.
    Stored,
    /// No object has the name: the store is the activation's own.
    Unscoped,
    /// The objects' shapes alone do not decide, or the store is not a plain one: it is left to
    /// the general path before any effect.
    Undecided,
}

/// `value` stored to `atom` in the scope `scope` of the window activation.
fn scoped_store(vm: &mut Vm, scope: &WindowScope, atom: Atom, value: Value) -> ScopedStore {
    match vm.window_with_binding(scope, atom) {
        None => ScopedStore::Undecided,
        Some(None) => ScopedStore::Unscoped,
        Some(Some((object, _))) if vm.set_plain_named_property(object, atom, value).is_some() => ScopedStore::Stored,
        Some(Some(_)) => ScopedStore::Undecided,
    }
}

/// The closure of `function` a window activation makes at `ip`: over its environment, which a
/// frame's promotion would make, carrying the activation's dynamic bindings
/// (`window_environment`) and, at a binding site, the site's scope
/// (`Vm::site_environment`), and capturing the `with` objects it entered
/// (`Vm::closure_over_scope`). Out of line, as `window_closure_value_in_lane`.
#[inline(never)]
fn scoped_closure_value(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    function: u32,
    registers: u16,
) -> Option<Value> {
    let scope = window_scope(cx, r, ip, true)?;
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    // The environment takes over the record's bindings, as a frame's promotion takes its own.
    let bindings = scope.record_bindings().map(std::mem::take);
    // SAFETY: the running window activation's entries start at `registers`, and its
    // environment's parent is the context's.
    let environment = unsafe {
        window_environment(vm, code, r.0.add(usize::from(registers)), scope.function, (*cx).registers.env(), scope.program, (*cx).registers.this(), bindings)
    };
    let site = scope.site.filter(|pc| vm.site_has_bindings(scope.program, scope.function, *pc));
    let value = vm
        .site_environment(environment, site)
        .and_then(|environment| vm.closure_over_scope(code, function, environment, vm.realm.globals, &scope.chain))
        .ok();
    note_allocation(vm);
    value
}

/// `ScopeReads::name` of the lane: the by-name read of `atom` through the window's scope into
/// `destination` and, for a call, `receiver`; `false` where the scope does not answer it. `site` is
/// the load's global-read cache field (`LOAD_NAME`'s `c`).
///
/// # Safety
/// `cx` is the running lane's context, and `r` its register file, live for the row's step.
pub(crate) unsafe fn scoped_name_into(cx: *mut (), r: Registers, ip: Ip, atom: u32,
    destination: Register, receiver: Option<Register>, site: u16) -> bool {
    let Some((value, receiver_value)) = scoped_name_read(cx.cast::<LaneContext>(), r, ip, atom, site) else {
        return false;
    };
    r.set(destination, value);
    if let Some(receiver) = receiver {
        r.set(receiver, receiver_value);
    }
    true
}

/// `ScopeReads::store` of the lane: `value` stored to `atom` in the `with` object the window's scope holds it in.
///
/// # Safety
/// As `scoped_name_into`.
pub(crate) unsafe fn scoped_store_into(cx: *mut (), r: Registers, ip: Ip, atom: u32, value: u64) -> bool {
    let cx = cx.cast::<LaneContext>();
    let vm = unsafe { vm(cx) };
    let Some(scope) = window_scope(cx, r, ip, true) else {
        return false;
    };
    matches!(scoped_store(vm, &scope, atom, Value::from_bits(value)), ScopedStore::Stored)
}

/// `ScopeReads::local` of the lane: `value` stored to local `slot` in the `with` object that has the local's name.
///
/// # Safety
/// As `scoped_name_into`.
pub(crate) unsafe fn scoped_local_store_into(cx: *mut (), r: Registers, ip: Ip, slot: u16, value: u64) -> bool {
    let cx = cx.cast::<LaneContext>();
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let vm = unsafe { vm(cx) };
    let Some(scope) = window_scope(cx, r, ip, false) else {
        return false;
    };
    let Some(&atom) = code.function(scope.function as usize).local_atoms.get(usize::from(slot)) else {
        return false;
    };
    matches!(scoped_store(vm, &scope, atom, Value::from_bits(value)), ScopedStore::Stored)
}

/// `ScopeReads::closure` of the lane: the closure of `function` over the window's scope, as a frame's
/// run makes it where the running activation keeps no environment of its own.
///
/// # Safety
/// As `scoped_name_into`.
pub(crate) unsafe fn scoped_closure_into(cx: *mut (), r: Registers, ip: Ip, function: u32, window: u16) -> Option<Value> {
    let cx = cx.cast::<LaneContext>();
    if unsafe { (*cx).registers }.env().is_deleted() {
        return closure_value_of_in_lane(cx, function);
    }
    scoped_closure_value(cx, r, ip, function, window)
}
