//! Names a window activation resolves through its scope: the `with` objects of its chain
//! (`LaneStack::with_chain`) and the dynamic bindings of its record (`FrameRecord`), which only a
//! suspended function has. A function that reaches names this way (`CallPlan::holds_window_scope`) runs
//! its by-name loads and stores, local stores and closures here instead of in a frame, where
//! `Vm::find_with_binding`, `Vm::load_name_call` and `Vm::closure_in_realm` make them. A lookup
//! the shapes of the objects alone do not decide, or that no `with` object or dynamic binding of
//! the activation answers, leaves the instruction to the general path before any effect.
use super::*;
use super::super::activation::FrameRecord;
use crate::bytecode::Atom;

/// What a name lookup reads of the window activation making it, in the terms of the frame
/// it stands for (`Vm::with_window_frame`).
#[derive(Clone, Copy)]
struct WindowScope {
    /// The activation's index on the lane stack.
    depth: usize,
    function: u32,
    program: ProgramId,
    /// The activation's environment: its own once a closure made one (`captured`), else the
    /// closure's.
    env: Value,
    captured: bool,
    /// The activation's record, null for an ordinary function's: it holds no dynamic bindings,
    /// which only a frame's direct eval makes.
    record: *mut FrameRecord,
    /// The resume pc of the binding site the access is at (`Frame::binding_site_pc`), if it is
    /// at one.
    site: Option<u32>,
}

/// The scope of the innermost activation, when it is an environment window: registers `r` and
/// the record at `ip` of its function are the access's. `at_site` where the access is an
/// instruction a binding site scopes (`Frame::binding_site_pc`). `None` in a frame's run.
fn window_scope<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, at_site: bool) -> Option<WindowScope> {
    let vm = unsafe { vm(cx) };
    let depth = vm.lane_stack.activations.len().checked_sub(1)?;
    let activation = vm.lane_stack.activations.last()?;
    let script = activation.script().filter(|script| script.environment)?;
    // SAFETY: the context's program outlives the lane run, and `ip` is a record of the running
    // function's view.
    let (code, pc) = unsafe { (&*(*cx).code, ip.offset_from((*cx).base) as usize) };
    let own = r.get(code.function(activation.function as usize).registers + WINDOW_ENVIRONMENT);
    let captured = !own.is_deleted();
    Some(WindowScope {
        depth,
        function: activation.function,
        program: unsafe { (*cx).program },
        env: if captured { own } else { unsafe { (*cx).env } },
        captured,
        record: script.record,
        site: at_site.then_some(pc as u32 + 1),
    })
}

impl WindowScope {
    /// The dynamic bindings the activation's record holds, until a closure's environment takes
    /// them over (`captured`); none for an activation without a record.
    fn record_bindings(&self) -> Option<&mut Vec<(Atom, Value)>> {
        // SAFETY: the record is the running activation's own, which nothing else accesses.
        (!self.captured && !self.record.is_null()).then(|| unsafe { &mut (*self.record).dynamic_bindings })
    }
}

impl<H: Host> Vm<H> {
    /// The `with` objects that precede the binding of `atom` for the window activation `scope`
    /// (`with_objects_before_binding`), computed over a frame that stands for the activation:
    /// the lookup reads the program, function, environment, binding site, dynamic bindings
    /// and scope chain of its frame, and runs no guest code, so the frame lives for it alone.
    fn with_window_frame(&mut self, scope: &WindowScope, chain: &[Value], atom: Atom) -> Vec<Value> {
        let mut frame = self.pooled_frame();
        frame.program = scope.program;
        frame.function = scope.function;
        frame.env = scope.env;
        frame.captured = scope.captured;
        frame.binding_site_pc = scope.site;
        if let Some(bindings) = scope.record_bindings() {
            frame.dynamic_bindings = std::mem::take(bindings);
        }
        frame.with_base = self.with_stack.len();
        self.with_stack.extend_from_slice(chain);
        self.frames.push(frame);
        let objects = self.with_objects_before_binding(self.frames.len() - 1, atom);
        let mut frame = self.frames.pop().expect("the frame the lookup pushed");
        self.with_stack.truncate(frame.with_base);
        if let Some(bindings) = scope.record_bindings() {
            *bindings = std::mem::take(&mut frame.dynamic_bindings);
        }
        self.pool_frame(frame);
        objects
    }

    /// The innermost object of `scope`'s chain that has `atom`, with its value there, as
    /// `find_with_binding` finds it for a frame: `Some(None)` where none does, `None` where the
    /// objects' shapes alone do not decide.
    fn window_with_binding(&mut self, scope: &WindowScope, atom: Atom) -> Option<Option<(Value, Value)>> {
        if !self.with_scope_entered || self.atom_class(atom).contains(AtomClass::INTERNAL) {
            return Some(None);
        }
        let chain: Vec<Value> = self.lane_stack.with_chain(scope.depth).collect();
        if chain.is_empty() {
            return Some(None);
        }
        let objects = self.with_window_frame(scope, &chain, atom);
        self.plain_first_with_binding(&objects, atom)
    }

    /// The value of the dynamic binding `atom` of the window activation `scope` holds itself (a
    /// direct eval's `var`): its environment's once a closure made one, else its record's.
    fn window_dynamic_binding(&self, scope: &WindowScope, atom: Atom) -> Option<Value> {
        let bindings: &[(Atom, Value)] = if scope.captured {
            self.heap.environment_bindings(scope.env)?
        } else {
            scope.record_bindings().map_or(&[], |bindings| bindings.as_slice())
        };
        bindings.iter().rev().find(|(candidate, _)| *candidate == atom).map(|(_, value)| *value)
    }
}

/// The read of `atom` the window activation at `ip` makes by name, as `Vm::load_name_call`
/// makes it: its value and its call's receiver, the `with` object that has it, else the
/// activation's own dynamic binding. `None`, before any effect, leaves it to the general path.
fn scoped_name_read<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, atom: Atom) -> Option<(Value, Value)> {
    let vm = unsafe { vm(cx) };
    if vm.atom_class(atom).contains(AtomClass::INTERNAL) {
        return None;
    }
    let scope = window_scope(cx, r, ip, true)?;
    if let Some((object, value)) = vm.window_with_binding(&scope, atom)? {
        return Some((value, object));
    }
    vm.window_dynamic_binding(&scope, atom).filter(|value| !value.is_deleted()).map(|value| (value, Value::UNDEFINED))
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
fn scoped_store<H: Host>(vm: &mut Vm<H>, scope: &WindowScope, atom: Atom, value: Value) -> ScopedStore {
    match vm.window_with_binding(scope, atom) {
        None => ScopedStore::Undecided,
        Some(None) => ScopedStore::Unscoped,
        Some(Some((object, _))) if vm.set_plain_named_property(object, atom, value).is_some() => ScopedStore::Stored,
        Some(Some(_)) => ScopedStore::Undecided,
    }
}

script_handler! {
    /// A by-name read in a function whose window activations resolve names through their scope
    /// (`lane_load_name_first`): `scoped_name_read`. Any other run, or a name that scope does not
    /// answer, takes the general path. `KIND` is the load's (`LOAD_NAME`).
    #[inline(never)]
    fn lane_scoped_load_name<KIND: u8>(cx, r, ip, i, view) {
        let Some((value, receiver)) = scoped_name_read(cx, r, ip, i.imm()) else {
            return exit(ip);
        };
        r.set(i.a, value);
        if KIND == LOAD_NAME_CALL {
            r.set(i.b, receiver);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// A by-name store in such a function (`lane_store_name_first`): to the `with` object that
    /// has the name (`scoped_store`); any other store takes the general path.
    #[inline(never)]
    fn lane_scoped_store_name<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let stored = window_scope(cx, r, ip, true)
            .is_some_and(|scope| matches!(scoped_store(vm, &scope, i.imm(), r.get(i.a)), ScopedStore::Stored));
        if !stored {
            return exit(ip);
        }
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `StoreEnvLocal` of such a function (`specialize_environment_window`), which may write a
    /// `with` object's binding instead of the local: the object that has the local's name takes
    /// the store (`scoped_store`), else the window's own (`lane_store_window_local`). A frame's
    /// run takes the general path, as it did for every function that resolves names at run time.
    #[inline(never)]
    fn lane_store_scoped_local<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        let (slot, _) = environment_window_fields(i);
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let vm = unsafe { vm(cx) };
        let Some(scope) = window_scope(cx, r, ip, false) else {
            return exit(ip);
        };
        let Some(&atom) = code.function(scope.function as usize).local_atoms.get(slot) else {
            return exit(ip);
        };
        let value = r.get(i.a);
        match scoped_store(vm, &scope, atom, value) {
            ScopedStore::Undecided => exit(ip),
            ScopedStore::Unscoped if ENVIRONMENT => sibling!(lane_store_window_local::<H, true>, r, ip, view),
            ScopedStore::Unscoped => sibling!(lane_store_window_local::<H, false>, r, ip, view),
            ScopedStore::Stored => {
                if let Some(register) = i.b.checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS) {
                    r.set(register, value);
                }
                next!(cx, r, following(ip), view)
            }
        }
    }
}

script_handler! {
    /// `MakeClosure` of such a function (`specialize_environment_window`): in a frame's run, as
    /// `lane_make_closure`; in a window activation, over its environment and its scope chain
    /// (`lane_scoped_closure_value`).
    fn lane_make_scoped_closure<>(cx, r, ip, i, view) {
        let (function, registers) = environment_window_fields(i);
        let value = if unsafe { (*cx).env }.is_deleted() {
            lane_closure_value_of(cx, function as u32)
        } else {
            lane_scoped_closure_value(cx, r, ip, function as u32, registers)
        };
        let Some(value) = value else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The closure of `function` a window activation makes at `ip`: over its environment, which a
/// frame's promotion would make, carrying the activation's dynamic bindings
/// (`window_environment`) and, at a binding site, the site's scope
/// (`Vm::site_environment`), and capturing the `with` objects it entered
/// (`Vm::closure_over_scope`). Out of line, as `lane_window_closure_value`.
#[inline(never)]
fn lane_scoped_closure_value<H: Host>(
    cx: *mut LaneContext<H>,
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
    let bindings = scope.record_bindings().map(std::mem::take).unwrap_or_default();
    // SAFETY: the running window activation's entries start at `registers`, and its
    // environment's parent is the context's.
    let environment = unsafe {
        window_environment(vm, code, r.0.add(usize::from(registers)), scope.function, (*cx).env, scope.program, (*cx).this, bindings)
    };
    let site = scope.site.filter(|pc| vm.site_has_bindings(scope.program, scope.function, *pc));
    let chain: Vec<Value> = vm.lane_stack.with_chain(scope.depth).collect();
    let value = vm
        .site_environment(environment, site)
        .and_then(|environment| vm.closure_over_scope(code, function, environment, vm.realm.globals, &chain))
        .ok();
    note_allocation(vm);
    value
}

/// The by-name read (`lane_scoped_load_name`) of a load of kind `KIND`.
pub(super) const fn scoped_load_name<H: Host, const KIND: u8>() -> Handler {
    lane_scoped_load_name::<H, KIND>
}

/// The by-name store (`lane_scoped_store_name`).
pub(super) const fn scoped_store_name<H: Host>() -> Handler {
    lane_scoped_store_name::<H>
}

/// The `StoreEnvLocal` (`lane_store_scoped_local`) of a window carrying its locals' slot storage
/// (`ENVIRONMENT`) or its locals alone.
pub(super) const fn scoped_store_local<H: Host, const ENVIRONMENT: bool>() -> Handler {
    lane_store_scoped_local::<H, ENVIRONMENT>
}

/// The `MakeClosure` (`lane_make_scoped_closure`).
pub(super) const fn scoped_closure<H: Host>() -> Handler {
    lane_make_scoped_closure::<H>
}
