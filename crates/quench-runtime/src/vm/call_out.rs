//! The VM operations staged rows call out for (`staged::CallOut`), and the interpreter's frame
//! that runs those rows in place. The lane's helper (`dispatch_fast::lane_vm_step`) makes the same
//! step by `Vm::call_out`, so every tier shares the one definition.

use super::*;
use crate::bytecode::{Operand, Register, WideInstruction};
use crate::staged::{CallOut, Concrete, Constant, Edge, Frame, GlobalFrame, NoPlans, NumericFrame, OperandValues, Operation, PropertyFrame, Protocol, Step};

/// What the global-name rows did in a test: the call-outs that ran, and the direct global stores
/// and typeofs the lane specializer emitted instead of one.
#[cfg(test)]
pub(crate) mod counts {
    use std::cell::Cell;

    thread_local! {
        pub(crate) static GLOBAL_CALL_OUTS: Cell<usize> = const { Cell::new(0) };
        pub(crate) static DIRECT_GLOBAL_STORES: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn bump(counter: &'static std::thread::LocalKey<Cell<usize>>) {
        counter.with(|count| count.set(count.get() + 1));
    }

    pub(crate) fn take(counter: &'static std::thread::LocalKey<Cell<usize>>) -> usize {
        counter.with(|count| count.replace(0))
    }
}

/// Where a call-out runs: the owner of the running activation's variables and registers, the
/// function whose code holds the instruction (the strictness a refusal throws by), and the PC
/// after its instruction, which the binding rules of a name read for their error site.
#[derive(Clone, Copy)]
pub(super) struct CallSite {
    pub(super) owner: VarOwner,
    pub(super) function: u32,
    pub(super) resume: usize,
}

impl<H: Host> Vm<H> {
    /// The VM's operation `operation` at `site` of its operand values (`CallOut::operands`): the
    /// value it defines, if any. A for-in step defines `Value::DELETED` once no key is left.
    pub(super) fn call_out(&mut self, p: &ResidualProgram, site: CallSite, operation: CallOut,
        operands: OperandValues<Value>) -> Result<Option<Value>, JsError> {
        let OperandValues { fixed: [first, second], list } = operands;
        let operand = second.unwrap_or(Value::UNDEFINED);
        #[cfg(test)]
        if matches!(operation, CallOut::GlobalWrite { .. } | CallOut::GlobalTypeof { .. }) {
            counts::bump(&counts::GLOBAL_CALL_OUTS);
        }
        match operation {
            CallOut::PropertyKey { .. } => self.to_property_key(p, operand).map(Some),
            CallOut::Equality { .. } | CallOut::Binary { .. } | CallOut::Delete { .. } | CallOut::Decide { .. } =>
                self.operator_step(p, site, operation, (first, operand)),
            CallOut::ForInNext { .. } => Ok(Some(self.for_in_next(p, operand)?.unwrap_or(Value::DELETED))),
            CallOut::Protocol { step, .. } => self.protocol_step(p, step, first, second),
            CallOut::Spread { passing, .. } => self.spread_call(p, site, passing, [first, second], &list).map(Some),
            CallOut::SuperCall { passing, .. } => self.super_call(p, passing, first, &list).map(Some),
            _ => self.name_step(p, site, operation, first),
        }
    }

    /// The operator steps of `call_out` over the operands `(left, right)`: equality, a binary
    /// operator, a delete and a branch decision.
    fn operator_step(&mut self, p: &ResidualProgram, site: CallSite, operation: CallOut, (left, right): (Option<Value>, Value))
        -> Result<Option<Value>, JsError> {
        let left = left.expect("an operator names its left operand");
        match operation {
            CallOut::Equality { strict, negated, .. } => {
                let equal = if strict { self.strict_equal(left, right) } else { self.equal(p, left, right)? };
                Ok(Some(Value::boolean(equal != negated)))
            }
            CallOut::Binary { operator, .. } => self.binary(p, operator, left, right).map(Some),
            CallOut::Delete { strict, .. } => {
                let strict = strict || p.function(site.function as usize).strict;
                self.delete_reference_property_mode(p, left, right, strict).map(Some)
            }
            CallOut::Decide { operator, .. } => self.binary_truthy(p, operator, left, right).map(|holds| Some(Value::boolean(holds))),
            _ => unreachable!("{operation:?} is no operator step"),
        }
    }

    /// The name, environment and arguments steps of `call_out`, by the binding rules of the
    /// running activation; `value` is the operand a store takes.
    fn name_step(&mut self, p: &ResidualProgram, site: CallSite, operation: CallOut, value: Option<Value>)
        -> Result<Option<Value>, JsError> {
        let CallSite { owner, resume, .. } = site;
        let value = value.unwrap_or(Value::UNDEFINED);
        match operation {
            CallOut::LoadEnvLocal { slot } => self.load_local_binding(p, owner, usize::from(slot), None).map(Some),
            CallOut::StoreEnvLocal { slot, initialize, .. } => {
                Ok(self.store_env_local(p, owner, usize::from(slot), value, initialize)?.then_some(value))
            }
            CallOut::ArgumentsLength { slot } => {
                let frame = self.own_frame();
                let binding = self.load_local_binding(p, VarOwner::Frame(frame), usize::from(slot), None)?;
                self.read_arguments_binding(p, frame, binding, None).map(Some)
            }
            CallOut::GlobalWrite { atom, site, initialize, .. } => {
                self.at_binding_site(owner, resume, |vm| vm.store_name(p, owner, atom, value, site, initialize))?;
                Ok(None)
            }
            CallOut::GlobalTypeof { atom, site } =>
                self.at_binding_site(owner, resume, |vm| vm.load_name_typeof(p, owner, atom, site)).map(Some),
            CallOut::LoadName { atom, site } =>
                self.at_binding_site(owner, resume, |vm| vm.load_name(p, owner, atom, Some(site))).map(Some),
            _ => unreachable!("{operation:?} is no name step"),
        }
    }

    /// The store of `value` to environment local `slot` of the activation `owner` names: a `with`
    /// scope's binding first, then the initialization, constant and function-name rules, a global
    /// `var`'s property, and at last the activation's binding with its mirrors. Whether the store
    /// defines the instruction's result: a sloppy assignment to a function's own name does not.
    pub(super) fn store_env_local(&mut self, p: &ResidualProgram, owner: VarOwner, slot: usize, value: Value,
        initialize: bool) -> Result<bool, JsError> {
        let (program, running) = (self.owner_view(owner).program(), self.owner_view(owner).function());
        let function = p.function(running as usize);
        let atom = function.local_atoms.get(slot).copied();
        if let Some(atom) = atom
            && self.store_with_binding(p, owner, atom, value, function.strict)?
        {
            return Ok(true);
        }
        self.check_local_assignment_initialized(p, owner, slot, initialize)?;
        if let Some(atom) = atom {
            self.refuse_immutable_store(p, function, atom, initialize)?;
        }
        if function.is_self_binding_slot(slot) {
            return if function.strict { Err(self.type_error(p, "assignment to function name binding".into())) } else { Ok(false) };
        }
        if let Some(atom) = self.root_global_var_atom(p, program, running, slot) {
            self.store_global_var(p, atom, value, function.strict)?;
            return Ok(true);
        }
        self.mirror_global_lexical_binding(p, owner, slot, value);
        *self.owner_binding_mut(p, owner, slot).ok_or_else(|| JsError("invalid local environment".into()))? = value;
        self.mapped_argument_store(p, owner, slot, value);
        Ok(true)
    }

    /// The refusal of a store to a global immutable binding after its initialization.
    fn refuse_immutable_store(&mut self, p: &ResidualProgram, function: &crate::bytecode::Function, atom: Atom, initialize: bool) -> Result<(), JsError> {
        if function.declares_global_immutable(atom) && !initialize {
            return Err(self.type_error(p, "assignment to constant binding".into()));
        }
        Ok(())
    }

    /// The store of `value` to the global `var` `atom`, which is the global object's property.
    fn store_global_var(&mut self, p: &ResidualProgram, atom: Atom, value: Value, strict: bool) -> Result<(), JsError> {
        let globals = self.realm.globals;
        let written = self.set_property_with_receiver(p, globals, atom, value, globals)?;
        if !written && strict {
            return Err(self.type_error(p, "cannot assign to read-only global binding".into()));
        }
        Ok(())
    }

    /// The running function's own frame, for the steps that read state only a frame has (its
    /// `arguments` object). A step made beside frameless activations (`lane_in_place`) has the
    /// caller's frame innermost: the activations become frames first, as guest code would make them.
    fn own_frame(&mut self) -> usize {
        self.materialize_deferred_lane();
        self.frames.len() - 1
    }

    /// The owner a call-out of `operation` made by a lane's helper runs for: the running
    /// activation where the step reads its names or locals (`CallOut::names_activation`), else
    /// the innermost frame.
    pub(super) fn call_out_owner(&mut self, operation: CallOut) -> VarOwner {
        if operation.names_activation() { self.running_owner() } else { VarOwner::Frame(self.frames.len().saturating_sub(1)) }
    }

    /// `action` with `resume` as the activation's binding-site PC, restored after.
    pub(super) fn at_binding_site<T>(&mut self, owner: VarOwner, resume: usize, action: impl FnOnce(&mut Self) -> T) -> T {
        let previous = self.replace_binding_site(owner, Some(resume as u32));
        let result = action(self);
        self.replace_binding_site(owner, previous);
        result
    }

    /// An iteration or spread `step` of its operand values: the value it defines, if any
    /// (`AppendArray` defines none).
    fn protocol_step(&mut self, p: &ResidualProgram, step: Protocol, array: Option<Value>, operand: Option<Value>)
        -> Result<Option<Value>, JsError> {
        let source = operand.unwrap_or(Value::UNDEFINED);
        match step {
            Protocol::GetIterator => self.get_iterator(p, source).map(Some),
            Protocol::ForInStart => self.for_in_start(p, source).map(Some),
            Protocol::IteratorClose => self.iterator_close(p, source).map(Some),
            Protocol::ArrayPatternFits { count } => Ok(Some(Value::boolean(self.array_pattern_fits(source, count)))),
            Protocol::AppendArray { length, spread } => {
                let array = array.expect("AppendArray names its array");
                self.append_array_literal_holes(array, length)?;
                match operand {
                    Some(value) if spread => self.spread_into_array(p, array, value)?,
                    Some(value) if self.appends_in_room(array, value, length) => {}
                    Some(value) => self.append_array_literal_elements(p, array, [value])?,
                    None => {}
                }
                Ok(None)
            }
        }
    }

    /// An `AppendArray` of one element after no elisions (`holes`) made by the staged row the b-jit
    /// runs (`elements::append_in_room`): the array's capacity word proves a slot, so nothing grows.
    #[inline(always)]
    fn appends_in_room(&mut self, array: Value, value: Value, holes: usize) -> bool {
        holes == 0
            && self.heap.array_record_base().is_some_and(|records| crate::staged::elements::concrete_append_in_room(records, array, value))
    }

    /// The call-out row `i` (`ToPropertyKey`, `GetIterator`, `ForInStart`, `IteratorClose`,
    /// `AppendArray`, `Delete`) at `site`, by its staged row on the interpreter's stage: the words
    /// decide a string key, and the VM makes every other step.
    pub(super) fn staged_value_step(&mut self, p: &ResidualProgram, site: CallSite, i: WideInstruction) -> Result<(), JsError> {
        let row = Operation::<u64, ()>::decode(i, crate::staged::DecodeContext::of(p)).expect("a value step decodes");
        self.staged_row(p, site, row)
    }

    /// The name read `i` (`LoadName`) at `site`, by its staged row on the interpreter's stage: the
    /// interpreter proves nothing of a global, so the VM's binding rules read it.
    pub(super) fn staged_load_name(&mut self, p: &ResidualProgram, site: CallSite, i: WideInstruction) -> Result<(), JsError> {
        let row = Operation::<u64, ()>::decode_load_name(i, ()).expect("a name read decodes");
        self.staged_row(p, site, row)
    }

    /// `row` on the interpreter's stage over the running frame: an error the VM raised is thrown.
    fn staged_row(&mut self, p: &ResidualProgram, site: CallSite, row: Operation<u64, ()>) -> Result<(), JsError> {
        let mut frame = HostFrame { vm: self, program: p, site, raised: None };
        match crate::staged::step(&mut Concrete::new(std::ptr::null()), &mut frame, row) {
            Step::Unresolved => Err(frame.raised.expect("a step left to the VM raised")),
            Step::Value(_) | Step::Control(_) => Ok(()),
        }
    }
}

/// The interpreter's frame for staged rows: the running frame's live registers, and the VM that
/// answers a row's call-out in place.
struct HostFrame<'a, H: Host> {
    vm: &'a mut Vm<H>,
    program: &'a ResidualProgram,
    site: CallSite,
    /// What the VM raised, for the step's caller to throw.
    raised: Option<JsError>,
}

impl<H: Host> Frame<Concrete> for HostFrame<'_, H> {
    type Fact = ();
    type Control = Edge;

    fn fact(&self, _register: Register) {}

    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        self.vm.owner_register(self.site.owner, register).to_bits()
    }

    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: ()) {
        self.vm.set_owner_register(self.site.owner, register, Value::from_bits(value));
    }

    fn this(&mut self, _stage: &mut Concrete) -> u64 { Value::DELETED.to_bits() }

    fn define_this(&mut self, stage: &mut Concrete, destination: Register, this: u64) {
        self.write(stage, destination, this, ());
    }

    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, ()) { (constant.value.to_bits(), ()) }

    fn truthiness(&mut self, _stage: &mut Concrete, _register: Register) -> Option<bool> { None }

    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> Edge {
        if condition { Edge::Target } else { Edge::Following }
    }

    fn jump(&mut self, _stage: &mut Concrete) -> Edge { Edge::Target }

    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) {
        (Value::boolean(condition).to_bits(), ())
    }

    fn vm_step(&mut self, _stage: &mut Concrete, operation: CallOut, _result: Option<Register>) -> Option<(u64, ())> {
        let (vm, owner) = (&*self.vm, self.site.owner);
        let operands = operation.operands().read(&self.program.method_arguments, |register| vm.owner_register(owner, register));
        match self.vm.call_out(self.program, self.site, operation, operands) {
            Ok(value) => Some((value.map_or(0, Value::to_bits), ())),
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }
}

impl<H: Host> GlobalFrame<Concrete> for HostFrame<'_, H> {}

impl<H: Host> PropertyFrame<Concrete> for HostFrame<'_, H> {
    type Plans = NoPlans;

    fn plans(&self) -> NoPlans { NoPlans }
}

impl<H: Host> crate::staged::ElementFrame<Concrete> for HostFrame<'_, H> {}

impl<H: Host> crate::staged::CallFrame<Concrete> for HostFrame<'_, H> {}

impl<H: Host> NumericFrame<Concrete> for HostFrame<'_, H> {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<u64> {
        unreachable!("a call-out row has no numeric operand")
    }

    fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<u64> {
        unreachable!("a call-out row has no numeric operand")
    }

    fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { None }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}
