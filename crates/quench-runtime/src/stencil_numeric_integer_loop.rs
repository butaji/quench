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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntegerRecurrence {
    Index,
    Constant(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntegerLoopProfile {
    Numeric,
    Affine,
}

impl IntegerRecurrence {
    fn physical_key(self) -> crate::stencil_fact::RegionKey {
        match self {
            Self::Index => crate::stencil_select::numeric_integer_loop_region_key(),
            Self::Constant(_) => crate::stencil_select::affine_i32_loop_region_key(),
        }
    }

    const fn constant_addend(self) -> i32 {
        match self {
            Self::Index => 0,
            Self::Constant(value) => value,
        }
    }

    const fn backedge(self) -> usize {
        match self {
            Self::Index => INDEX_LOOP_BACKEDGE,
            Self::Constant(_) => CONSTANT_LOOP_BACKEDGE,
        }
    }

    const fn region_end(self) -> usize {
        match self {
            Self::Index => INDEX_REGION_END,
            Self::Constant(_) => CONSTANT_REGION_END,
        }
    }

    const fn profile(self) -> IntegerLoopProfile {
        match self {
            Self::Index => IntegerLoopProfile::Numeric,
            Self::Constant(_) => IntegerLoopProfile::Affine,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct IntegerLoopSelection {
    pub(crate) state_slot: u16,
    pub(crate) value_slot: u16,
    pub(crate) index_slot: u16,
    pub(crate) multiplier: i32,
    pub(crate) recurrence: IntegerRecurrence,
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
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>>,
}

impl NativeIntegerLoopPlan {
    pub(crate) fn new(
        selection: IntegerLoopSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.affine_i32_loops.then_some(())?;
        let view = crate::stencil_select::select_physical_for_abi(
            selection.recurrence.physical_key(),
            crate::stencil_select::RegionAbi::AffineI32Loop,
        )?;
        (view.generated && view.executable && view.stencil.validate()).then_some(())?;
        let site = crate::quickening::QuickeningSite::<4>::new(Opcode::Binary);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let image = crate::stencil_region_layout::finalize_selected_leaf(view, &values).ok()?;
        Some(Self {
            selection,
            owner,
            image,
            cache: crate::stencil_select::RenderedRegionCache::new(),
            installed: None,
        })
    }

    pub(crate) fn execute(
        &mut self,
        code: CodeView<'_>,
        environment: &crate::environment::Environment,
        vm: &crate::vm::VmContext,
    ) -> Result<Option<IntegerLoopOutcome>, NativeDispatchError> {
        let Some((seed, end)) = self.inputs(code, environment) else {
            return Ok(None);
        };
        let mut context = IntegerLoopContext {
            index: 0,
            end,
            value: seed,
            multiplier: self.selection.multiplier,
            addend: self.selection.recurrence.constant_addend(),
            _padding: 0,
            interrupt: vm.interrupt_flag(),
        };
        let status = self.invoke(&mut context)?;
        self.finish(status, context, environment, vm).map(Some)
    }

    fn inputs(
        &self,
        code: CodeView<'_>,
        environment: &crate::environment::Environment,
    ) -> Option<(i32, usize)> {
        environment
            .with_proven_object(self.selection.state_slot, |object| {
                let seed = crate::vm::cached_own_property_number(code, 1, object)?;
                let end = crate::vm::cached_own_property_number(code, 9, object)?;
                let seed = exact_i32(seed)?;
                let end = exact_bound(end)?;
                exact_for_all_iterations(seed, self.selection, end)?;
                Some((seed, end))
            })
            .flatten()
    }

    fn invoke(&mut self, context: &mut IntegerLoopContext) -> Result<u64, NativeDispatchError> {
        let entry = self.entry()?;
        let lease = crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry)
            .map_err(|error| {
                NativeDispatchError::Physical(format!("integer loop lease: {error:?}"))
            })?;
        lease
            .invoke(|call| call((context as *mut IntegerLoopContext).cast()))
            .map_err(|error| {
                NativeDispatchError::Physical(format!("integer loop invoke: {error:?}"))
            })
    }

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
                pc: LOOP_HEADER,
                profile: self.selection.recurrence.profile(),
            });
        }
        if status != crate::vm::NATIVE_DISPATCH_OK || context.index != context.end {
            return Err(NativeDispatchError::committed(
                self.selection.recurrence.backedge(),
                "integer recurrence returned incomplete progress",
            ));
        }
        Ok(IntegerLoopOutcome::Completed {
            value: context.value,
            next: self.selection.recurrence.region_end(),
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

    fn entry(
        &mut self,
    ) -> Result<
        crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>,
        NativeDispatchError,
    > {
        if let Some(entry) = self
            .installed
            .filter(|entry| self.owner.borrow().entry_token_is_live(*entry))
        {
            return Ok(entry);
        }
        self.installed = None;
        let address = self
            .owner
            .borrow_mut()
            .publish_region_image_or_get(&mut self.cache, &self.image)
            .map_err(|error| {
                NativeDispatchError::Physical(format!("integer loop publish: {error:?}"))
            })?;
        let entry = self
            .owner
            .borrow()
            .owned_affine_i32_loop_entry(address)
            .map_err(|error| {
                NativeDispatchError::Physical(format!("integer loop entry: {error:?}"))
            })?;
        self.installed = Some(entry);
        Ok(entry)
    }
}

pub(crate) use crate::stencil_numeric_integer_selection::select_integer_loop;
