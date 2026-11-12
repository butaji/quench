//! The VM operations staged rows call out for (`staged::CallOut`), and the interpreter's frame
//! that runs those rows in place. The lane's helper (`dispatch_fast::lane_vm_step`) makes the same
//! step by `Vm::call_out`, so every tier shares the one definition.

use super::*;
use crate::bytecode::{Operand, Register, WideInstruction};
use crate::staged::{CallOut, Concrete, Constant, Edge, Frame, GlobalFrame, NumericFrame, Operation, Protocol, Step};

/// Where a call-out runs: the running frame, and the PC after its instruction, which the
/// binding rules of a name read for their error site.
#[derive(Clone, Copy)]
pub(super) struct CallSite {
    pub(super) frame: usize,
    pub(super) resume: usize,
}

impl<H: Host> Vm<H> {
    /// The VM's operation `operation` at `site` of its operand values (`CallOut::operands`): the
    /// value it defines, if any. A for-in step defines `Value::DELETED` once no key is left.
    pub(super) fn call_out(&mut self, p: &ResidualProgram, site: CallSite, operation: CallOut,
        [first, second]: [Option<Value>; 2]) -> Result<Option<Value>, JsError> {
        let operand = second.unwrap_or(Value::UNDEFINED);
        match operation {
            CallOut::PropertyKey { .. } => self.to_property_key(p, operand).map(Some),
            CallOut::Equality { strict, negated, .. } => {
                let left = first.expect("an equality names its left operand");
                let equal = if strict { self.strict_equal(left, operand) } else { self.equal(p, left, operand)? };
                Ok(Some(Value::boolean(equal != negated)))
            }
            CallOut::Binary { operator, .. } =>
                self.binary(p, operator, first.expect("a binary names its left operand"), operand).map(Some),
            CallOut::ForInNext { .. } => Ok(Some(self.for_in_next(p, operand)?.unwrap_or(Value::DELETED))),
            CallOut::Protocol { step, .. } => self.protocol_step(p, step, first, second),
            _ => self.name_step(p, site, operation, first),
        }
    }

    /// The name, environment and arguments steps of `call_out`, by the binding rules of the
    /// running frame; `value` is the operand a store takes.
    fn name_step(&mut self, p: &ResidualProgram, site: CallSite, operation: CallOut, value: Option<Value>)
        -> Result<Option<Value>, JsError> {
        let CallSite { frame, resume } = site;
        let value = value.unwrap_or(Value::UNDEFINED);
        match operation {
            CallOut::LoadEnvLocal { slot } => self.load_local_binding(p, frame, usize::from(slot), None).map(Some),
            CallOut::StoreEnvLocal { slot, initialize, .. } => {
                self.store_env_local(p, frame, usize::from(slot), value, initialize)?;
                Ok(Some(value))
            }
            CallOut::ArgumentsLength { slot } => {
                let binding = self.load_local_binding(p, frame, usize::from(slot), None)?;
                self.read_arguments_binding(p, frame, binding, None).map(Some)
            }
            CallOut::GlobalWrite { atom, site, initialize, .. } => {
                self.at_binding_site(frame, resume, |vm| vm.store_name(p, atom, value, site, initialize))?;
                Ok(None)
            }
            CallOut::GlobalTypeof { atom, site } =>
                self.at_binding_site(frame, resume, |vm| vm.load_name_typeof(p, atom, site)).map(Some),
            _ => unreachable!("{operation:?} is no name step"),
        }
    }

    /// The store of `value` to environment local `slot` of frame `f`: a `with` scope's binding
    /// first, then the initialization, constant and function-name rules, a global `var`'s property,
    /// and at last the frame's binding with its mirrors.
    pub(super) fn store_env_local(&mut self, p: &ResidualProgram, f: usize, slot: usize, value: Value,
        initialize: bool) -> Result<(), JsError> {
        let function = p.function(self.frames[f].function as usize);
        let atom = function.local_atoms.get(slot).copied();
        if let Some(atom) = atom
            && self.store_with_binding(p, f, atom, value, function.strict)?
        {
            return Ok(());
        }
        self.check_local_assignment_initialized(p, f, slot, initialize)?;
        if let Some(atom) = atom
            && function.declares_global_immutable(atom)
            && !initialize
        {
            return Err(self.type_error(p, "assignment to constant binding".into()));
        }
        if function.is_self_binding_slot(slot) {
            if function.strict {
                return Err(self.type_error(p, "assignment to function name binding".into()));
            }
            return Ok(());
        }
        let global_var = self.root_global_var_atom(p, self.frames[f].program, self.frames[f].function, slot);
        if let Some(atom) = global_var {
            let globals = self.realm.globals;
            let written = self.set_property_with_receiver(p, globals, atom, value, globals)?;
            if !written && function.strict {
                return Err(self.type_error(p, "cannot assign to read-only global binding".into()));
            }
            return Ok(());
        }
        self.mirror_global_lexical_binding(p, f, slot, value);
        *self.frame_binding_mut(p, f, slot).ok_or_else(|| JsError("invalid local environment".into()))? = value;
        self.mapped_argument_store(p, f, slot, value);
        Ok(())
    }

    /// `action` with `resume` as the frame's binding-site PC, restored after.
    fn at_binding_site<T>(&mut self, frame: usize, resume: usize, action: impl FnOnce(&mut Self) -> T) -> T {
        let previous = self.frames[frame].binding_site_pc.replace(resume as u32);
        let result = action(self);
        self.frames[frame].binding_site_pc = previous;
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
            Protocol::AppendArray { length, spread } => {
                let array = array.expect("AppendArray names its array");
                self.append_array_literal_holes(array, length)?;
                match operand {
                    Some(value) if spread => self.spread_into_array(p, array, value)?,
                    Some(value) => self.append_array_literal_elements(p, array, [value])?,
                    None => {}
                }
                Ok(None)
            }
        }
    }

    /// The call-out row `i` (`ToPropertyKey`, `GetIterator`, `ForInStart`, `IteratorClose`,
    /// `AppendArray`) at `site`, by its staged row on the interpreter's stage: the words
    /// decide a string key, and the VM makes every other step.
    pub(super) fn staged_value_step(&mut self, p: &ResidualProgram, site: CallSite, i: WideInstruction) -> Result<(), JsError> {
        let row = Operation::<u64, ()>::decode(i, |_| None).expect("a value step decodes");
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
        self.vm.read(self.site.frame, register).to_bits()
    }

    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: ()) {
        self.vm.write(self.site.frame, register, Value::from_bits(value));
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
        let operands = operation.operands().map(|register| register.map(|register| self.vm.read(self.site.frame, register)));
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

impl<H: Host> NumericFrame<Concrete> for HostFrame<'_, H> {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> u64 {
        unreachable!("a call-out row has no numeric operand")
    }

    fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> u64 {
        unreachable!("a call-out row has no numeric operand")
    }

    fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { None }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}
