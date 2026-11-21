//! The state an activation lends the lookups that read its scope: by-name resolution, `with`
//! scopes, `new.target` and `this` bindings, and the direct eval its code makes. One trait over
//! where that state lives, a VM frame or the window of a lane activation
//! (`dispatch_fast::scoped_names::WindowScope`), so each lookup reads the same facts of either
//! and no lookup needs a frame fabricated for an activation that has none.
use super::*;
use super::activation::FrameRecord;

/// What a lookup reads of the activation it runs for. Whatever else is derived of these (the
/// function's strictness, its home atom, its private names) is read of the function's metadata
/// by `function`, and of the dynamic bindings by `Vm::dynamic_binding`.
pub(in crate::vm) trait CallerView {
    /// The program the activation runs.
    fn program(&self) -> ProgramId;

    /// The function of `program` it runs.
    fn function(&self) -> u32;

    /// Its receiver as the activation holds it: `DELETED` where a lexical binding decides it.
    fn this(&self) -> Value;

    /// Its environment word: the activation's own environment where `owns_environment`, else
    /// the closure's, which the activation captured nothing over.
    fn environment(&self) -> Value;

    /// Whether `environment` is the activation's own, made by its first closure, which holds
    /// its dynamic bindings from then on.
    fn owns_environment(&self) -> bool;

    /// The resume pc of the binding site its current operation is at, if it is at one.
    fn binding_site(&self) -> Option<u32>;

    /// The dynamic bindings the activation holds itself: a direct eval's `var`s, the lexical
    /// `this`, `new.target`. Empty once its environment holds them (`owns_environment`).
    fn held_bindings(&self) -> &[(Atom, Value)];

    /// The `with` objects of its scope chain, outermost first, among `entered`, the scope
    /// stack of the VM's frames, which a frame's chain is a suffix of.
    fn scope_chain<'a>(&'a self, entered: &'a [Value]) -> &'a [Value];

}

impl CallerView for Frame {
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
        self.binding_site_pc
    }

    fn held_bindings(&self) -> &[(Atom, Value)] {
        &self.dynamic_bindings
    }

    fn scope_chain<'a>(&'a self, entered: &'a [Value]) -> &'a [Value] {
        &entered[self.with_base.min(entered.len())..]
    }
}

/// The record of a lane activation, which a direct eval it runs promotes its environment into
/// (`lane_eval`): the scope chain is the `with` objects the eval entered it with.
impl CallerView for FrameRecord {
    fn program(&self) -> ProgramId {
        self.program
    }

    fn function(&self) -> u32 {
        self.function
    }

    fn this(&self) -> Value {
        self.this()
    }

    fn environment(&self) -> Value {
        self.env
    }

    fn owns_environment(&self) -> bool {
        self.captured
    }

    fn binding_site(&self) -> Option<u32> {
        self.binding_site_pc()
    }

    fn held_bindings(&self) -> &[(Atom, Value)] {
        self.dynamic_bindings()
    }

    fn scope_chain<'a>(&'a self, _entered: &'a [Value]) -> &'a [Value] {
        self.with_objects()
    }
}

impl Vm {
    /// The view of the frame at `index` of the VM's stack.
    pub(super) fn frame_caller(&self, index: usize) -> &dyn CallerView {
        &*self.frames[index]
    }

    /// The dynamic bindings `caller` owns: its environment's once that holds them, else its own.
    pub(super) fn own_dynamic_bindings<'a>(&'a self, caller: &'a dyn CallerView) -> Option<&'a [(Atom, Value)]> {
        if caller.owns_environment() {
            self.heap.environment_bindings(caller.environment()).map(Vec::as_slice)
        } else {
            Some(caller.held_bindings())
        }
    }

    /// The environment `caller`'s closure captured: its own environment's parent once it has made
    /// one, else the environment word itself.
    pub(super) fn captured_parent_environment(&self, caller: &dyn CallerView) -> Value {
        if !caller.owns_environment() {
            return caller.environment();
        }
        match self.heap.environment(caller.environment()) {
            Some(crate::heap::EnvironmentRecord { parent, .. }) => *parent,
            _ => Value::NULL,
        }
    }

    /// The environment `depth` function layers out from `caller`'s captures (`Vm::capture_env`).
    pub(super) fn caller_capture_env(&self, caller: &dyn CallerView, depth: u16) -> Option<Value> {
        self.environment_capture_env(caller.environment(), caller.owns_environment(), depth)
    }

    /// The activation a direct eval runs for (`eval_owner`), `None` for host code.
    pub(super) fn eval_caller(&self) -> Option<&dyn CallerView> {
        self.eval_owner().map(|owner| self.owner_view(owner))
    }

    /// The metadata in `p` of the function the eval's caller runs.
    pub(super) fn eval_caller_function<'p>(&self, p: &'p ResidualProgram) -> Option<&'p crate::bytecode::Function> {
        p.function_get(self.eval_caller()?.function() as usize)
    }
}
