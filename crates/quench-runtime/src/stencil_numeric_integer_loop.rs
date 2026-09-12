//! Guarded integer recurrence over one canonical lowered counting loop.

use crate::ir::Opcode;
use crate::machine::{CodeView, NativeDispatchError};
use crate::stencil_numeric_integer_selection::{exact_bound, exact_for_all_iterations, exact_i32};
use std::{cell::RefCell, rc::Rc};

pub(crate) const INDEX_REGION_END: usize = 31;
pub(crate) const CONSTANT_REGION_END: usize = 30;
pub(crate) const LOOP_HEADER: usize = 7;
pub(crate) const INDEX_LOOP_BACKEDGE: usize = 26;
pub(crate) const INDEX_LOOP_EXIT: usize = 27;
pub(crate) const CONSTANT_LOOP_BACKEDGE: usize = 25;
pub(crate) const CONSTANT_LOOP_EXIT: usize = 26;
pub(crate) const NAMED_REGION_END: usize = 28;
pub(crate) const NAMED_LOOP_BACKEDGE: usize = 23;
pub(crate) const POLYMORPHIC_REGION_END: usize = 37;
pub(crate) const POLYMORPHIC_LOOP_BACKEDGE: usize = 32;
pub(crate) const DIRECT_REGION_END: usize = 30;
pub(crate) const DIRECT_LOOP_BACKEDGE: usize = 25;
pub(crate) const DIRECT_LOOP_HEADER: usize = 10;
pub(crate) const DIRECT_LOOP_EXIT: usize = 26;
pub(crate) const RECEIVER_REGION_END: usize = 33;
pub(crate) const RECEIVER_LOOP_BACKEDGE: usize = 28;
pub(crate) const BOUND_REGION_END: usize = 33;
pub(crate) const BOUND_LOOP_BACKEDGE: usize = 28;
pub(crate) const ARGUMENTS_REGION_END: usize = 39;
pub(crate) const ARGUMENTS_LOOP_BACKEDGE: usize = 34;
/// Current canonical lowering for a local integer recurrence. The loop starts
/// with its seed/index initialization and re-enters at the bound test.
pub(crate) const LOCAL_REGION_END: usize = 29;
pub(crate) const LOCAL_LOOP_HEADER: usize = 6;
pub(crate) const LOCAL_LOOP_BACKEDGE: usize = 24;
pub(crate) const LOCAL_LOOP_EXIT: usize = 25;
/// Loop-header view used when OSR enters the local recurrence after its
/// initialization prefix has already committed.  The span starts at the
/// bound test and ends at the canonical value/undefined returns.
pub(crate) const LOCAL_BODY_REGION_END: usize = 23;
pub(crate) const LOCAL_BODY_LOOP_BACKEDGE: usize = 18;
pub(crate) const LOCAL_BODY_LOOP_EXIT: usize = 19;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum IntegerRecurrence {
    Index,
    Constant(i32),
    LocalConstant(i32),
    LocalConstantBody(i32),
    NamedCallee(std::rc::Rc<str>),
    DirectCallee(std::rc::Rc<str>),
    EquivalentCallees([std::rc::Rc<str>; 2]),
    ReceiverConstant(i32),
    BoundCallee(std::rc::Rc<str>),
    ArgumentConstants(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntegerLoopProfile {
    Numeric,
    Affine,
    CallsInline,
    CallsDirect,
    CallsChanging,
    CallsReceiver,
    CallsBound,
    CallsArguments,
}

impl IntegerRecurrence {
    fn physical_key(&self) -> crate::stencil_fact::RegionKey {
        match self {
            Self::Index => crate::stencil_select::numeric_integer_loop_region_key(),
            Self::Constant(_)
            | Self::LocalConstant(_)
            | Self::LocalConstantBody(_)
            | Self::NamedCallee(_)
            | Self::DirectCallee(_)
            | Self::EquivalentCallees(_)
            | Self::ReceiverConstant(_) => crate::stencil_select::affine_i32_loop_region_key(),
            Self::BoundCallee(_) => crate::stencil_select::affine_i32_loop_region_key(),
            Self::ArgumentConstants(_) => crate::stencil_select::affine_i32_loop_region_key(),
        }
    }

    fn static_formula(&self, multiplier: i32) -> Option<(i32, i32)> {
        match self {
            Self::Index => Some((multiplier, 0)),
            Self::Constant(value) => Some((multiplier, *value)),
            Self::LocalConstant(value) => Some((multiplier, *value)),
            Self::LocalConstantBody(value) => Some((multiplier, *value)),
            Self::ReceiverConstant(value) => Some((multiplier, *value)),
            Self::ArgumentConstants(value) => Some((multiplier, *value)),
            Self::NamedCallee(_)
            | Self::DirectCallee(_)
            | Self::EquivalentCallees(_)
            | Self::BoundCallee(_) => None,
        }
    }

    const fn backedge_offset(&self) -> usize {
        match self {
            Self::Index => INDEX_LOOP_BACKEDGE,
            Self::Constant(_) => CONSTANT_LOOP_BACKEDGE,
            Self::LocalConstant(_) => LOCAL_LOOP_BACKEDGE,
            Self::LocalConstantBody(_) => LOCAL_BODY_LOOP_BACKEDGE,
            Self::NamedCallee(_) => NAMED_LOOP_BACKEDGE,
            Self::DirectCallee(_) => DIRECT_LOOP_BACKEDGE,
            Self::EquivalentCallees(_) => POLYMORPHIC_LOOP_BACKEDGE,
            Self::ReceiverConstant(_) => RECEIVER_LOOP_BACKEDGE,
            Self::BoundCallee(_) => BOUND_LOOP_BACKEDGE,
            Self::ArgumentConstants(_) => ARGUMENTS_LOOP_BACKEDGE,
        }
    }

    const fn loop_header_offset(&self) -> usize {
        match self {
            Self::DirectCallee(_) => DIRECT_LOOP_HEADER,
            Self::Index | Self::Constant(_) | Self::NamedCallee(_) | Self::EquivalentCallees(_) => {
                LOOP_HEADER
            }
            Self::LocalConstant(_) => LOCAL_LOOP_HEADER,
            Self::LocalConstantBody(_) => 0,
            Self::ReceiverConstant(_) => 12,
            Self::BoundCallee(_) => 13,
            Self::ArgumentConstants(_) => 14,
        }
    }

    const fn region_end_offset(&self) -> usize {
        match self {
            Self::Index => INDEX_REGION_END,
            Self::Constant(_) => CONSTANT_REGION_END,
            Self::LocalConstant(_) => LOCAL_REGION_END,
            Self::LocalConstantBody(_) => LOCAL_BODY_REGION_END,
            Self::NamedCallee(_) => NAMED_REGION_END,
            Self::DirectCallee(_) => DIRECT_REGION_END,
            Self::EquivalentCallees(_) => POLYMORPHIC_REGION_END,
            Self::ReceiverConstant(_) => RECEIVER_REGION_END,
            Self::BoundCallee(_) => BOUND_REGION_END,
            Self::ArgumentConstants(_) => ARGUMENTS_REGION_END,
        }
    }

    const fn profile(&self) -> IntegerLoopProfile {
        match self {
            Self::Index => IntegerLoopProfile::Numeric,
            Self::Constant(_) => IntegerLoopProfile::Affine,
            Self::LocalConstant(_) | Self::LocalConstantBody(_) => IntegerLoopProfile::Affine,
            Self::NamedCallee(_) => IntegerLoopProfile::CallsInline,
            Self::DirectCallee(_) => IntegerLoopProfile::CallsDirect,
            Self::EquivalentCallees(_) => IntegerLoopProfile::CallsChanging,
            Self::ReceiverConstant(_) => IntegerLoopProfile::CallsReceiver,
            Self::BoundCallee(_) => IntegerLoopProfile::CallsBound,
            Self::ArgumentConstants(_) => IntegerLoopProfile::CallsArguments,
        }
    }
}

#[derive(Clone)]
pub(crate) struct IntegerLoopSelection {
    pub(crate) state_slot: u16,
    pub(crate) value_slot: u16,
    pub(crate) index_slot: u16,
    pub(crate) seed_pc: usize,
    pub(crate) bound_pc: usize,
    pub(crate) multiplier: i32,
    pub(crate) recurrence: IntegerRecurrence,
    pub(crate) loop_header_pc: usize,
    pub(crate) backedge_pc: usize,
    pub(crate) region_end_pc: usize,
}

impl IntegerLoopSelection {
    pub(crate) fn at(
        start: usize,
        state_slot: u16,
        value_slot: u16,
        index_slot: u16,
        seed_offset: usize,
        bound_offset: usize,
        multiplier: i32,
        recurrence: IntegerRecurrence,
    ) -> Option<Self> {
        Some(Self {
            state_slot,
            value_slot,
            index_slot,
            seed_pc: start.checked_add(seed_offset)?,
            bound_pc: start.checked_add(bound_offset)?,
            multiplier,
            loop_header_pc: start.checked_add(recurrence.loop_header_offset())?,
            backedge_pc: start.checked_add(recurrence.backedge_offset())?,
            region_end_pc: start.checked_add(recurrence.region_end_offset())?,
            recurrence,
        })
    }
}

struct IntegerLoopInputs {
    /// The committed induction value at entry. Full-loop admission starts at
    /// zero; the loop-header OSR view carries the live local index forward.
    index: usize,
    seed: i32,
    end: usize,
    multiplier: i32,
    addend: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum IntegerLoopOutcome {
    Completed {
        value: i32,
        next: usize,
        profile: IntegerLoopProfile,
    },
    Resume {
        pc: usize,
        profile: IntegerLoopProfile,
    },
}

#[repr(C)]
struct IntegerLoopContext {
    index: usize,
    end: usize,
    value: i32,
    multiplier: i32,
    addend: i32,
    _padding: u32,
    interrupt: *const std::sync::atomic::AtomicBool,
}

pub(crate) struct NativeIntegerLoopPlan {
    selection: IntegerLoopSelection,
    machine: AffineMachinePlan,
}

struct AffineMachinePlan {
    image: crate::stencil_region_layout::VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<crate::stencil_arena::DispatchEntry>,
}

impl NativeIntegerLoopPlan {
    pub(crate) fn new(
        selection: IntegerLoopSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.affine_i32_loops.then_some(())?;
        let machine = AffineMachinePlan::new(selection.recurrence.physical_key(), owner)?;
        Some(Self { selection, machine })
    }

    pub(crate) fn execute(
        &mut self,
        code: CodeView<'_>,
        environment: &crate::environment::Environment,
        vm: &crate::vm::VmContext,
    ) -> Result<Option<IntegerLoopOutcome>, NativeDispatchError> {
        let Some(inputs) = self.inputs(code, environment) else {
            return Ok(None);
        };
        let mut context = IntegerLoopContext {
            index: inputs.index,
            end: inputs.end,
            value: inputs.seed,
            multiplier: inputs.multiplier,
            addend: inputs.addend,
            _padding: 0,
            interrupt: vm.interrupt_flag(),
        };
        let status = self.machine.invoke(&mut context)?;
        self.finish(status, context, environment, vm).map(Some)
    }

    fn inputs(
        &self,
        code: CodeView<'_>,
        environment: &crate::environment::Environment,
    ) -> Option<IntegerLoopInputs> {
        if let IntegerRecurrence::LocalConstant(addend)
        | IntegerRecurrence::LocalConstantBody(addend) = &self.selection.recurrence
        {
            let addend = *addend;
            let index = Self::initial_index(
                &self.selection.recurrence,
                self.selection.index_slot,
                environment,
            )?;
            let seed = crate::stencil_numeric_integer_selection::exact_i32(
                environment.get_number(self.selection.value_slot)?,
            )?;
            let end = environment.with_proven_object(self.selection.state_slot, |object| {
                crate::vm::cached_own_property_number(code, self.selection.bound_pc, object)
                    .and_then(crate::stencil_numeric_integer_selection::exact_bound)
            })??;
            (index <= end).then_some(())?;
            crate::stencil_numeric_integer_selection::exact_for_all_iterations(
                seed,
                self.selection.multiplier,
                addend,
                end,
            )?;
            return Some(IntegerLoopInputs {
                index,
                seed,
                end,
                multiplier: self.selection.multiplier,
                addend,
            });
        }
        environment
            .with_proven_object(self.selection.state_slot, |object| {
                let seed =
                    crate::vm::cached_own_property_number(code, self.selection.seed_pc, object)?;
                let end =
                    crate::vm::cached_own_property_number(code, self.selection.bound_pc, object)?;
                let seed = exact_i32(seed)?;
                let end = exact_bound(end)?;
                let (multiplier, addend) = self.formula(object)?;
                exact_for_all_iterations(seed, multiplier, addend, end)?;
                Some(IntegerLoopInputs {
                    index: 0,
                    seed,
                    end,
                    multiplier,
                    addend,
                })
            })
            .flatten()
    }

    fn initial_index(
        recurrence: &IntegerRecurrence,
        index_slot: u16,
        environment: &crate::environment::Environment,
    ) -> Option<usize> {
        if matches!(recurrence, IntegerRecurrence::LocalConstantBody(_)) {
            return crate::stencil_numeric_integer_selection::exact_bound(
                environment.get_number(index_slot)?,
            );
        }
        Some(0)
    }

    fn formula(&self, object: &crate::value::ObjectData) -> Option<(i32, i32)> {
        if let Some(formula) = self
            .selection
            .recurrence
            .static_formula(self.selection.multiplier)
        {
            return Some(formula);
        }
        match &self.selection.recurrence {
            IntegerRecurrence::NamedCallee(key) => affine_callee_formula(object, key),
            IntegerRecurrence::DirectCallee(key) => affine_callee_formula(object, key),
            IntegerRecurrence::BoundCallee(key) => {
                let function = own_function(object, key)?;
                intrinsic_bind_is_current(&function).then_some(())?;
                affine_function_formula(&function)
            }
            IntegerRecurrence::EquivalentCallees(keys) => {
                let first = affine_callee_formula(object, &keys[0])?;
                (affine_callee_formula(object, &keys[1])? == first).then_some(first)
            }
            IntegerRecurrence::Index
            | IntegerRecurrence::Constant(_)
            | IntegerRecurrence::LocalConstant(_)
            | IntegerRecurrence::LocalConstantBody(_)
            | IntegerRecurrence::ReceiverConstant(_)
            | IntegerRecurrence::ArgumentConstants(_) => None,
        }
    }
}

impl AffineMachinePlan {
    fn new(
        key: crate::stencil_fact::RegionKey,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let view = crate::stencil_select::select_physical_for_abi(
            key,
            crate::stencil_select::RegionAbi::AffineI32Loop,
        )?;
        (view.generated && view.executable && view.stencil.validate()).then_some(())?;
        let site = crate::quickening::QuickeningSite::<4>::new(Opcode::Binary);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let image = crate::stencil_region_layout::finalize_selected_leaf(view, &values).ok()?;
        Some(Self {
            image,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
        })
    }

    fn invoke(&mut self, context: &mut IntegerLoopContext) -> Result<u64, NativeDispatchError> {
        let entry = self.entry()?;
        self.physical
            .invoke(entry, |call| {
                call((context as *mut IntegerLoopContext).cast())
            })
            .map_err(|error| {
                NativeDispatchError::Physical(format!("integer loop invoke: {error:?}"))
            })
    }

    fn entry(
        &mut self,
    ) -> Result<
        crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>,
        NativeDispatchError,
    > {
        self.physical
            .entry(
                |owner, cache| {
                    owner
                        .borrow_mut()
                        .publish_region_image_or_get(cache, &self.image)
                },
                |pool, address| pool.owned_affine_i32_loop_entry(address),
            )
            .map_err(|error| {
                NativeDispatchError::Physical(format!("integer loop entry: {error:?}"))
            })
    }
}

impl NativeIntegerLoopPlan {
    fn finish(
        &self,
        status: u64,
        context: IntegerLoopContext,
        environment: &crate::environment::Environment,
        vm: &crate::vm::VmContext,
    ) -> Result<IntegerLoopOutcome, NativeDispatchError> {
        if status == crate::vm::NATIVE_DISPATCH_INTERRUPT && context.index < context.end {
            vm.clear_interrupt();
            self.commit(&context, environment);
            return Ok(IntegerLoopOutcome::Resume {
                pc: self.selection.loop_header_pc,
                profile: self.selection.recurrence.profile(),
            });
        }
        if status != crate::vm::NATIVE_DISPATCH_OK || context.index != context.end {
            return Err(NativeDispatchError::committed(
                self.selection.backedge_pc,
                "integer recurrence returned incomplete progress",
            ));
        }
        Ok(IntegerLoopOutcome::Completed {
            value: context.value,
            next: self.selection.region_end_pc,
            profile: self.selection.recurrence.profile(),
        })
    }

    fn commit(&self, context: &IntegerLoopContext, environment: &crate::environment::Environment) {
        environment.set(
            self.selection.index_slot,
            crate::value::Value::Number(context.index as f64),
        );
        environment.set(
            self.selection.value_slot,
            crate::value::Value::Number(f64::from(context.value)),
        );
    }
}

pub(crate) struct NativeAffineI32Transform {
    machine: AffineMachinePlan,
}

impl NativeAffineI32Transform {
    pub(crate) fn new(owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>) -> Option<Self> {
        AffineMachinePlan::new(crate::stencil_select::affine_i32_loop_region_key(), owner)
            .map(|machine| Self { machine })
    }

    pub(crate) fn execute(&mut self, input: i32, multiplier: i32, addend: i32) -> Option<i32> {
        static NO_INTERRUPT: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        input.checked_mul(multiplier)?.checked_add(addend)?;
        let mut context = IntegerLoopContext {
            index: 0,
            end: 1,
            value: input,
            multiplier,
            addend,
            _padding: 0,
            interrupt: &NO_INTERRUPT,
        };
        let status = self.machine.invoke(&mut context).ok()?;
        (status == crate::vm::NATIVE_DISPATCH_OK && context.index == 1).then_some(context.value)
    }
}

fn affine_callee_formula(object: &crate::value::ObjectData, key: &str) -> Option<(i32, i32)> {
    let function = own_function(object, key)?;
    affine_function_formula(&function)
}

fn affine_function_formula(function: &crate::value::FunctionValue) -> Option<(i32, i32)> {
    crate::functions::direct_call_eligible(&function).then_some(())?;
    let fact = function.code.numeric_affine_i32()?;
    (usize::from(fact.parameter_slot) == function.captures.len())
        .then_some((fact.multiplier, fact.addend))
}

fn intrinsic_bind_is_current(function: &crate::value::FunctionValue) -> bool {
    let properties = function.properties.borrow();
    if properties.iter().any(|(name, _)| name == "bind") {
        return false;
    }
    let prototype = properties.iter().rev().find_map(|(name, value)| {
        matches!(name.as_str(), "\0function_prototype" | "\0prototype").then_some(value)
    });
    let prototype = prototype.cloned().unwrap_or_else(|| {
        crate::vm::realm_intrinsic_for(
            crate::construct::function_realm_id(function),
            crate::ops::Builtin::FunctionPrototype,
        )
    });
    drop(properties);
    match prototype {
        crate::value::Value::Builtin(crate::ops::Builtin::FunctionPrototype) => matches!(
            crate::builtins::property(crate::ops::Builtin::FunctionPrototype, "bind"),
            crate::value::Value::Builtin(crate::ops::Builtin::FunctionBind)
        ),
        crate::value::Value::Object(object) => crate::vm::proven_own_word(&object, "bind")
            .is_some_and(|word| word.is_builtin(crate::ops::Builtin::FunctionBind)),
        _ => false,
    }
}

fn own_function(
    object: &crate::value::ObjectData,
    key: &str,
) -> Option<Rc<crate::value::FunctionValue>> {
    let pointer = crate::vm::proven_own_word(object, key)?.function_ptr()?;
    unsafe { Rc::increment_strong_count(pointer) };
    Some(unsafe { Rc::from_raw(pointer) })
}

#[cfg(test)]
mod tests {
    use super::{IntegerLoopSelection, IntegerRecurrence, NativeIntegerLoopPlan};

    #[test]
    fn selection_offsets_are_derived_from_cfg_start() {
        let selection = IntegerLoopSelection::at(17, 2, 3, 4, 1, 9, 33, IntegerRecurrence::Index)
            .expect("relative recipe offsets fit the code range");
        assert_eq!(selection.seed_pc, 18);
        assert_eq!(selection.bound_pc, 26);
        assert_eq!(selection.loop_header_pc, 24);
        assert_eq!(selection.backedge_pc, 43);
        assert_eq!(selection.region_end_pc, 48);
    }

    #[test]
    fn osr_loop_header_uses_live_index_and_full_entry_starts_at_zero() {
        let environment = crate::environment::Environment::new();
        environment.set(12, crate::value::Value::Number(3.0));
        assert_eq!(
            NativeIntegerLoopPlan::initial_index(
                &IntegerRecurrence::LocalConstantBody(7),
                12,
                &environment,
            ),
            Some(3)
        );
        assert_eq!(
            NativeIntegerLoopPlan::initial_index(
                &IntegerRecurrence::LocalConstant(7),
                12,
                &environment,
            ),
            Some(0)
        );
    }
}

pub(crate) use crate::stencil_numeric_integer_selection::select_integer_loop;
