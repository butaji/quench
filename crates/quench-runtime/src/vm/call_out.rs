//! The VM operations staged rows call out for (`staged::CallOut`), and the interpreter's frame
//! that runs those rows in place. The lane's helper (`dispatch_fast::vm_operation_step`) makes the same
//! step by `Vm::call_out`, so every tier shares the one definition.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt, OperationNamesExt};
use super::*;
use crate::bytecode::{FieldLookup, Operand, Register, WideInstruction};
use crate::staged::{CallOut, Concrete, Constant, Edge, Frame, GlobalFrame, NoPlans, NumericFrame, OperandValues, Operation, PropertyFrame, Protocol, Step};

/// `result`'s value, or `None` with its error kept in `raised`: how a VM frame's row (`HostFrame`,
/// `InterpreterFrame`) answers a VM check that may raise, which the row's runner then throws.
#[inline(always)]
pub(super) fn stash_raise<T>(raised: &mut Option<JsError>, result: Result<T, JsError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            *raised = Some(error);
            None
        }
    }
}

/// What the global-name rows did in a test: the call-outs that ran, and the direct global stores
/// and typeofs the lane specializer emitted instead of one.
#[cfg(any(test, feature = "test-support"))]
pub mod counts {
    use std::cell::Cell;

    thread_local! {
        pub static GLOBAL_CALL_OUTS: Cell<usize> = const { Cell::new(0) };
        pub static DIRECT_GLOBAL_STORES: Cell<usize> = const { Cell::new(0) };
        /// The receivers whose run of constant field defines was skipped as already held.
        pub static CONSTANT_FIELD_SKIPS: Cell<usize> = const { Cell::new(0) };
    }

    #[quench_macros::test_visible]
    pub(crate) fn bump(counter: &'static std::thread::LocalKey<Cell<usize>>) {
        counter.with(|count| count.set(count.get() + 1));
    }

    pub(crate) fn take(counter: &'static std::thread::LocalKey<Cell<usize>>) -> usize {
        counter.with(|count| count.replace(0))
    }
}

/// What a named or indexed read produced: its value, or the activation its getter pushed, which the
/// read's destination takes when that activation returns.
pub(super) enum Loaded {
    Value(Value),
    Pushed(StepResult),
}

/// Where a call-out runs: the owner of the running activation's variables and registers, the
/// function whose code holds the instruction (the strictness a refusal throws by), and the PC
/// after its instruction, which the binding rules of a name read for their error site.
#[derive(Clone, Copy)]
pub struct CallSite {
    pub owner: VarOwner,
    pub function: u32,
    pub resume: usize,
}

impl Vm {
    /// The VM's operation `operation` at `site` of its operand values (`CallOut::operands`): the
    /// value it defines, if any. A for-in step defines `Value::DELETED` once no key is left.
    pub fn call_out(&mut self, p: &ResidualProgram, site: CallSite, operation: CallOut,
        operands: OperandValues<Value>) -> Result<Option<Value>, JsError> {
        let OperandValues { fixed: [first, second], list } = operands;
        let operand = second.unwrap_or(Value::UNDEFINED);
        #[cfg(any(test, feature = "test-support"))]
        if matches!(operation, CallOut::GlobalWrite { .. } | CallOut::GlobalTypeof { .. }) {
            counts::bump(&counts::GLOBAL_CALL_OUTS);
        }
        match operation {
            CallOut::PropertyKey { .. } => self.to_property_key(p, operand).map(Some),
            CallOut::ToNumeric { .. } => self.to_numeric(p, operand).map(Some),
            CallOut::IncDec { decrement, .. } => self.inc_dec_value(p, operand, decrement).map(Some),
            CallOut::Unary { operator, .. } => self.unary(p, operator, operand).map(Some),
            // A read without a destination inlines no getter (`Loaded::Pushed` is the interpreter's own).
            CallOut::LoadField { lookup } => {
                let frame = self.owner_frame(site.owner).expect("a field read names its frame");
                match self.load_field_step(p, frame, site.resume, lookup, None, false)? {
                    Loaded::Value(value) => Ok(Some(value)),
                    Loaded::Pushed(_) => unreachable!("a read with no destination inlines no getter"),
                }
            }
            CallOut::LoadElement { .. } => {
                let frame = self.owner_frame(site.owner).expect("an element read names its frame");
                let base = first.expect("an element read names its base");
                match self.load_element_step(p, frame, site.resume, (base, operand), None, false)? {
                    Loaded::Value(value) => Ok(Some(value)),
                    Loaded::Pushed(_) => unreachable!("a read with no destination inlines no getter"),
                }
            }
            // A named store is the field store the row names, or the store to the frame's `this`: strict where the row
            // says so (no function's strictness).
            CallOut::StoreField { object, atom, site: cache, strict, .. } => {
                let object = match object {
                    Some(_) => first.expect("a store names its object"),
                    None => {
                        let frame = self.owner_frame(site.owner).expect("a store to this names its frame");
                        self.checked_this_binding(p, frame)?
                    }
                };
                self.set_field_cached(p, object, atom, operand, cache, strict).map(|()| None)
            }
            // A keyed store: the value is the operand list's register, and strict where the row or the function is.
            CallOut::StoreElement { strict, .. } => {
                let (object, value) = (first.expect("a keyed store names its object"), list.first().copied().expect("a keyed store names its value"));
                let strict = strict || p.function(site.function as usize).strict;
                self.set_index_mode(p, object, operand, value, strict).map(|()| None)
            }
            CallOut::Equality { .. } | CallOut::Binary { .. } | CallOut::Delete { .. } | CallOut::Decide { .. }
                | CallOut::DecideOperand { .. } => self.operator_step(p, site, operation, (first, operand)),
            CallOut::ForInNext { .. } => Ok(Some(self.for_in_next(p, operand)?.unwrap_or(Value::DELETED))),
            CallOut::EnumeratedGet { .. } => self.get_index(p, first.expect("an enumerated read names its base"), operand).map(Some),
            CallOut::Protocol { step: Protocol::RestView { get, fixed, rest }, .. } =>
                self.rest_view_step(p, site, (get, fixed, rest), first, second.unwrap_or(Value::UNDEFINED)).map(Some),
            CallOut::Protocol { step, .. } => self.protocol_step(p, site, step, first, second),
            CallOut::Spread { passing, .. } => self.spread_call(p, site, passing, [first, second], &list).map(Some),
            CallOut::SuperCall { passing, .. } => self.super_call(p, passing, first, &list).map(Some),
            CallOut::Accessor { atom, site: cache, .. } =>
                self.get_field_cached(p, first.expect("an accessor read names its receiver"), atom, cache).map(Some),
            _ => self.name_step(p, site, operation, first),
        }
    }

    /// The value of a call-out's operand `operand` read by `owner`: a register of the owner, a program constant,
    /// or the field or local the owner's frame names (`resolve_operand`).
    pub(super) fn call_out_operand(&mut self, p: &ResidualProgram, owner: VarOwner, operand: Operand) -> Result<Value, JsError> {
        match operand.kind() {
            Some(crate::bytecode::OperandKind::Register) => Ok(self.owner_register(owner, operand.payload())),
            Some(crate::bytecode::OperandKind::Constant) => {
                Ok(self.active_constant(usize::from(operand.payload())).expect("a call-out's constant is in its program"))
            }
            _ => {
                let frame = self.owner_frame(owner).expect("a call-out's field or local names its frame");
                self.resolve_operand(p, frame, operand)
            }
        }
    }

    /// The named read `lookup` of frame `frame` at resume PC `pc`: a nested chain's value, or the atom's value
    /// through its base. Where `destination` names the register the read writes and `allow_inline_calls` holds,
    /// a cached getter runs as an in-place activation (`try_inline_getter`), which pushes.
    pub(super) fn load_field_step(&mut self, p: &ResidualProgram, frame: usize, pc: usize, lookup: FieldLookup,
        destination: Option<Register>, allow_inline_calls: bool) -> Result<Loaded, JsError> {
        let (atom, base, cache_site) = match lookup {
            FieldLookup::Site(site) => return self.resolve_field(p, frame, site).map(Loaded::Value),
            FieldLookup::Atom { atom, base, cache_site } => (atom, base, cache_site),
        };
        let base = self.resolve_field_base(p, frame, base)?;
        // A read fused with its frame's return pushes the getter as a call whose completion returns from the frame
        // (`PendingGeneralCall::returns`); a getter is no tail call, so a strict frame pushes it too.
        if allow_inline_calls
            && let Some(destination) = destination
            && let Some(getter) = self.cached_field_getter(base, atom, self.field_cache_index(cache_site))
            && let Some(result) = self.try_inline_getter(p, frame, pc, getter, base, destination)
        {
            return result.map(Loaded::Pushed);
        }
        // A private field reads straight from the object's private elements; methods, accessors and a missing
        // brand take the ordinary [[Get]].
        match self.is_private_name(atom).then(|| self.private_field_value(p, base, atom)).flatten() {
            Some(value) => Ok(Loaded::Value(value)),
            None => self.get_field_cached(p, base, atom, cache_site).map(Loaded::Value),
        }
    }

    /// `base[key]` at resume PC `pc` of frame `frame`: a proved symbol key's getter runs in place where
    /// `destination` and `allow_inline_calls` hold (`load_field_step`), else the VM's indexed read.
    pub(super) fn load_element_step(&mut self, p: &ResidualProgram, frame: usize, pc: usize, (base, key): (Value, Value),
        destination: Option<Register>, allow_inline_calls: bool) -> Result<Loaded, JsError> {
        if allow_inline_calls
            && let Some(destination) = destination
            && let Some(getter) = self.proved_symbol_getter(base, key)
            && let Some(result) = self.try_inline_getter(p, frame, pc, getter, base, destination)
        {
            return result.map(Loaded::Pushed);
        }
        self.get_index(p, base, key).map(Loaded::Value)
    }

    /// `++` (or `--`) of `input`: the Number step, else ToNumeric and the step of the converted value
    /// (its BigInt update). The value the update defines.
    pub(super) fn inc_dec_value(&mut self, p: &ResidualProgram, input: Value, is_decrement: bool) -> Result<Value, JsError> {
        match super::dispatch::number_increment(input, is_decrement) {
            Some(value) => Ok(value),
            None => {
                let numeric = self.to_numeric(p, input)?;
                match super::dispatch::number_increment(numeric, is_decrement) {
                    Some(value) => Ok(value),
                    None => self.numeric_update(p, numeric, is_decrement),
                }
            }
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
            // ToBoolean of the general `Binary` result, which a constant operand's comparison takes as its decision.
            CallOut::DecideOperand { operator, .. } => {
                let value = self.binary(p, operator, left, right)?;
                Ok(Some(Value::boolean(self.truthy(value))))
            }
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
            // The element at `value` (the index operand), as the argument-get row reads it.
            CallOut::ArgumentGet { slot, .. } => {
                let frame = self.own_frame();
                let binding = self.load_local_binding(p, VarOwner::Frame(frame), usize::from(slot), None)?;
                self.read_arguments_binding(p, frame, binding, Some(value)).map(Some)
            }
            CallOut::GlobalWrite { atom, site, initialize, .. } => {
                self.at_binding_site(owner, resume, |vm| vm.store_name(p, owner, atom, value, site, initialize))?;
                Ok(None)
            }
            CallOut::GlobalTypeof { atom, site } =>
                self.at_binding_site(owner, resume, |vm| vm.load_name_typeof(p, owner, atom, site)).map(Some),
            CallOut::GlobalLexicalLoad { declaration, site } =>
                self.global_lexical_load(p, owner, (declaration, site), resume).map(Some),
            CallOut::GlobalLexicalStore { declaration, site, initialize, .. } => {
                self.global_lexical_store(p, owner, (declaration, site, initialize), value, resume)?;
                Ok(None)
            }
            CallOut::LoadName { atom, site } =>
                self.at_binding_site(owner, resume, |vm| vm.load_name(p, owner, atom, Some(site))).map(Some),
            // The frame's own clone, as the environment-clone row makes it and the lane's clone did: the frame's
            // environment becomes its iteration copy where a closure captured it. Defines no register.
            CallOut::CloneEnv { plan } => {
                let frame = self.owner_frame(owner).expect("a clone names its frame");
                let body = p.function(self.frames[frame].function as usize);
                let clones = &body.environment_clones;
                let clone = clones.get(plan as usize).expect("a clone's plan belongs to the running function");
                self.clone_frame_environment(frame, &clone.fresh, clones.resets(clone))?;
                Ok(None)
            }
            _ => unreachable!("{operation:?} is no name step"),
        }
    }

    /// The store of `value` to environment local `slot` of the activation `owner` names: a `with`
    /// scope's binding first, then the initialization, constant and function-name rules, a global
    /// `var`'s property, and at last the activation's binding. Whether the store
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
    /// `arguments` object). A step made beside frameless activations (`in_place_in_lane`) has the
    /// caller's frame innermost: the activations become frames first, as guest code would make them.
    pub(super) fn own_frame(&mut self) -> usize {
        self.materialize_deferred_lane();
        self.frames.len() - 1
    }

    /// The owner a call-out of `operation` made by a lane's helper runs for: the running
    /// activation where the step reads its names or locals (`CallOut::names_activation`), else
    /// the innermost frame.
    pub fn call_out_owner(&mut self, operation: CallOut) -> VarOwner {
        if operation.names_activation() { self.running_owner() } else { VarOwner::Frame(self.frames.len().saturating_sub(1)) }
    }

    /// `action` with `resume` as the activation's binding-site PC, restored after.
    pub(super) fn at_binding_site<T>(&mut self, owner: VarOwner, resume: usize, action: impl FnOnce(&mut Self) -> T) -> T {
        let previous = self.replace_binding_site(owner, Some(resume as u32));
        let result = action(self);
        self.replace_binding_site(owner, previous);
        result
    }

    /// An iteration or spread `step` of its operand values, or a receiver's constant fields
    /// test: the value it defines, if any (`AppendArray` defines none).
    fn protocol_step(&mut self, p: &ResidualProgram, site: CallSite, step: Protocol, array: Option<Value>, operand: Option<Value>)
        -> Result<Option<Value>, JsError> {
        let source = operand.unwrap_or(Value::UNDEFINED);
        match step {
            Protocol::GetIterator => self.get_iterator(p, source).map(Some),
            Protocol::ForInStart => self.for_in_start(p, source).map(Some),
            Protocol::IteratorClose => self.iterator_close(p, source).map(Some),
            Protocol::RestView { .. } => unreachable!("a rest view's step reads its activation (`rest_view_step`)"),
            Protocol::ConstantFieldsHeld { site } => Ok(Some(Value::boolean(self.constant_fields_held(source, site)))),
            Protocol::Literal(step) => self.literal_step(p, site, step, array, operand),
            Protocol::Class(step) => self.class_step(p, site, step, array, operand),
            Protocol::Value(step) => self.value_step(p, site, step, array, operand),
            Protocol::Owned(step) => self.owned_var_step(p, site, step, array, operand),
            Protocol::Wasm(step) => self.wasm_host_step(p, site, step, array, operand),
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
        self.staged_row(p, site, row, i.op())
    }

    /// The name read `i` (`LoadName`) at `site`, by its staged row on the interpreter's stage: the
    /// interpreter proves nothing of a global, so the VM's binding rules read it.
    pub(super) fn staged_load_name(&mut self, p: &ResidualProgram, site: CallSite, i: WideInstruction) -> Result<(), JsError> {
        let row = Operation::<u64, ()>::decode_load_name(i, ()).expect("a name read decodes");
        self.staged_row(p, site, row, i.op())
    }

    /// `row` on the interpreter's stage over the running frame: an error the VM raised is thrown.
    fn staged_row(&mut self, p: &ResidualProgram, site: CallSite, row: Operation<u64, ()>, op: crate::bytecode::Op) -> Result<(), JsError> {
        let mut stage = crate::staged::concrete_over_vm(self);
        let mut frame = HostFrame { vm: self, program: p, site, raised: None };
        match crate::staged::step(&mut stage, &mut frame, row) {
            Step::Unresolved => Err(frame.raised.unwrap_or_else(|| panic!("a step left to the VM raised: {op:?}"))),
            Step::Value(_) | Step::Control(_) => Ok(()),
        }
    }
}

/// The interpreter's frame for staged rows: the running frame's live registers, and the VM that
/// answers a row's call-out in place. Its suspension hooks are `suspension`'s.
pub(super) struct HostFrame<'a> {
    pub(super) vm: &'a mut Vm,
    pub(super) program: &'a ResidualProgram,
    pub(super) site: CallSite,
    /// What the VM raised, for the step's caller to throw.
    pub(super) raised: Option<JsError>,
}

/// Where a row on the interpreter's frame sends control: an edge, or a suspension of the running
/// activation (`Vm::staged_suspend`).
pub(super) enum HostControl {
    Edge(Edge),
    Suspend(crate::staged::Suspending<u64>),
}

impl Frame<Concrete> for HostFrame<'_> {
    type Fact = ();
    type Control = HostControl;

    fn fact(&self, _register: Register) {}

    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        self.vm.owner_register(self.site.owner, register).to_bits()
    }

    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: ()) {
        self.vm.set_owner_register(self.site.owner, register, Value::from_bits(value));
    }

    fn this(&mut self, _stage: &mut Concrete) -> u64 { Value::DELETED.to_bits() }

    /// The running activation's `this`, where bound; an unbound `this` (a TDZ read) raises, which the
    /// row's caller throws.
    fn this_value(&mut self, _stage: &mut Concrete) -> Option<(u64, ())> {
        let this = self.vm.owner_this(self.program, self.site.owner).expect("a this read names its frame");
        stash_raise(&mut self.raised, this).map(|this| (this.to_bits(), ()))
    }

    fn callee_value(&mut self, _stage: &mut Concrete) -> Option<(u64, ())> {
        Some((self.vm.owner_callee(self.site.owner).expect("a callee read names its frame").to_bits(), ()))
    }

    fn define_this(&mut self, stage: &mut Concrete, destination: Register, this: u64) {
        self.write(stage, destination, this, ());
    }

    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, ()) { (constant.value.to_bits(), ()) }

    fn truthiness(&mut self, _stage: &mut Concrete, _register: Register) -> Option<bool> { None }

    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> HostControl {
        HostControl::Edge(if condition { Edge::Target } else { Edge::Following })
    }

    fn jump(&mut self, _stage: &mut Concrete) -> HostControl { HostControl::Edge(Edge::Target) }

    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) {
        (Value::boolean(condition).to_bits(), ())
    }

    /// `RequireIteratorResult`'s object test: the VM's own check, which raises the iterator-result TypeError.
    fn object_like(&mut self, _stage: &mut Concrete, word: u64) -> Option<()> {
        let result = self.vm.require_iterator_result(self.program, Value::from_bits(word));
        stash_raise(&mut self.raised, result)
    }

    /// `RequireObjectCoercible`'s miss: the VM's own check, which raises for a `null` or `undefined` word.
    fn object_coercion_failed(&mut self, _stage: &mut Concrete, word: u64) {
        let result = self.vm.require_object_coercible(self.program, Value::from_bits(word));
        stash_raise(&mut self.raised, result);
    }

    /// `SetFunctionName` of `function` to the program atom `name` at cache `site`: the VM's naming.
    fn set_function_name(&mut self, _stage: &mut Concrete, function: u64, name: u32, site: u16) -> Option<()> {
        let result = self.vm.set_function_name(self.program, Value::from_bits(function), (name, site));
        stash_raise(&mut self.raised, result)
    }

    fn vm_step(&mut self, _stage: &mut Concrete, operation: CallOut, _result: Option<Register>) -> Option<(u64, ())> {
        let (vm, owner) = (&*self.vm, self.site.owner);
        let operands = operation.operands().read(&self.program.method_arguments, |operand| match operand.register_index() {
            Some(register) => vm.owner_register(owner, register),
            None => vm.active_constant(usize::from(operand.payload())).expect("a call-out's constant is in its program"),
        });
        match self.vm.call_out(self.program, self.site, operation, operands) {
            Ok(value) => Some((value.map_or(0, Value::to_bits), ())),
            Err(error) => {
                self.raised = Some(error);
                None
            }
        }
    }
}

impl GlobalFrame<Concrete> for HostFrame<'_> {}

impl PropertyFrame<Concrete> for HostFrame<'_> {
    type Plans = NoPlans;

    fn plans(&self) -> NoPlans { NoPlans }
}

impl crate::staged::ElementFrame<Concrete> for HostFrame<'_> {}

impl NumericFrame<Concrete> for HostFrame<'_> {
    fn int_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<crate::staged::Int32<u64>> {
        unreachable!("a call-out row has no numeric operand")
    }

    fn number_operand(&mut self, _stage: &mut Concrete, _operand: Operand, _miss: ()) -> Option<crate::staged::Float64<u64>> {
        unreachable!("a call-out row has no numeric operand")
    }

    fn prove_number(&mut self, _stage: &mut Concrete, _register: Register, _miss: ()) -> Option<()> { None }
    /// The host's registers hold tagged words, so an int32 a row computes is written as its
    /// tagged int (`IntCaseIndex`'s slot, which the branch table after it reads as an int).
    fn int_result(&mut self, _stage: &mut Concrete, word: u64) -> u64 { Value::integer(word as u32 as i32).to_bits() }
    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}

}
