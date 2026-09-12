//! Guarded ordered dense-Number reduction.

use crate::machine::{CodeView, NativeDispatchError};
use std::{cell::RefCell, rc::Rc};

#[path = "stencil_ordered_reduction_select.rs"]
mod selection;
#[path = "stencil_ordered_reduction_shapes.rs"]
mod shapes;
pub(crate) use selection::select_reduction;

#[repr(C)]
pub(crate) struct NativeReductionContext {
    source: *const f64,
    len: usize,
    index: usize,
    total: f64,
    interrupt: *const std::sync::atomic::AtomicBool,
    threshold: f64,
    on_true: f64,
    on_false: f64,
}

const _: () = {
    assert!(std::mem::align_of::<NativeReductionContext>() == 8);
    assert!(std::mem::offset_of!(NativeReductionContext, source) == 0);
    assert!(std::mem::offset_of!(NativeReductionContext, len) == 8);
    assert!(std::mem::offset_of!(NativeReductionContext, index) == 16);
    assert!(std::mem::offset_of!(NativeReductionContext, total) == 24);
    assert!(std::mem::offset_of!(NativeReductionContext, interrupt) == 32);
    assert!(std::mem::offset_of!(NativeReductionContext, threshold) == 40);
    assert!(std::mem::offset_of!(NativeReductionContext, on_true) == 48);
    assert!(std::mem::offset_of!(NativeReductionContext, on_false) == 56);
    assert!(std::mem::size_of::<NativeReductionContext>() == 64);
};

#[derive(Clone, Copy)]
pub(crate) struct ReductionSelection {
    source: ReductionSource,
    profile: ReductionProfile,
    operation: ReductionOperation,
    total_slot: u16,
    index_slot: u16,
    region_end: usize,
    loop_header: usize,
    loop_backedge: usize,
}

#[derive(Clone, Copy)]
enum ReductionProfile {
    OrderedF64,
    ControlFor,
    ControlWhile,
    Conditional,
}

#[derive(Clone, Copy)]
enum ReductionOperation {
    Sum,
    LessThan {
        threshold: f64,
        on_true: f64,
        on_false: f64,
    },
}

#[derive(Clone, Copy)]
enum ReductionSource {
    DirectArray {
        slot: u16,
    },
    StateArray {
        slot: u16,
        array_pc: usize,
        bound_pc: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ReductionOutcome {
    Completed(f64),
    Resume { pc: usize },
}

pub(crate) struct NativeReductionPlan {
    selection: ReductionSelection,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<crate::stencil_arena::DispatchEntry>,
}

impl NativeReductionPlan {
    pub(crate) fn new(
        selection: ReductionSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.array_numeric_loops.then_some(())?;
        let view = crate::stencil_select::select_physical(selection.operation.region_key())?;
        (view.abi == crate::stencil_select::RegionAbi::ArrayReductionLoop
            && view.executable
            && view.stencil.validate())
        .then_some(())?;
        Some(Self {
            selection,
            image: region_image(view),
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
        })
    }

    pub(crate) fn execute(
        &mut self,
        code: CodeView<'_>,
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
    ) -> Result<Option<ReductionOutcome>, NativeDispatchError> {
        if !environment.can_elide_terminal_store(self.selection.index_slot)
            || !environment.can_elide_terminal_store(self.selection.total_slot)
        {
            return Ok(None);
        }
        match self.selection.source {
            ReductionSource::DirectArray { slot } => environment
                .with_proven_array(slot, |array| {
                    self.execute_array(array, None, environment, context)
                })
                .unwrap_or(Ok(None)),
            ReductionSource::StateArray {
                slot,
                array_pc,
                bound_pc,
            } => self.execute_state(code, environment, context, slot, array_pc, bound_pc),
        }
    }

    fn execute_state(
        &mut self,
        code: CodeView<'_>,
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
        slot: u16,
        array_pc: usize,
        bound_pc: usize,
    ) -> Result<Option<ReductionOutcome>, NativeDispatchError> {
        let Some(object) = environment.retain_proven_object(slot) else {
            return Ok(None);
        };
        let Some(bound) =
            crate::vm::cached_own_property_number(code, bound_pc, &object).and_then(exact_bound)
        else {
            return Ok(None);
        };
        crate::vm::with_cached_own_property_array(code, array_pc, &object, |array| {
            self.execute_array(array, Some(bound), environment, context)
        })
        .unwrap_or(Ok(None))
    }

    fn execute_array(
        &mut self,
        array: &crate::value::ArrayData,
        bound: Option<usize>,
        environment: &crate::environment::Environment,
        context: &crate::vm::VmContext,
    ) -> Result<Option<ReductionOutcome>, NativeDispatchError> {
        if !crate::locals::array_word_is_current(array)
            || !array.is_packed_data()
            || !array.is_dense_numeric_data()
        {
            return Ok(None);
        }
        let words = array.numeric_kernel_words().ok_or_else(|| {
            NativeDispatchError::Physical("ordered reduction backing changed".into())
        })?;
        let len = bound.unwrap_or(words.len());
        if len > words.len() {
            return Ok(None);
        }
        let mut native = NativeReductionContext {
            source: words.as_ptr(),
            len,
            index: 0,
            total: 0.0,
            interrupt: context.interrupt_flag(),
            threshold: 0.0,
            on_true: 0.0,
            on_false: 0.0,
        };
        self.selection.operation.configure(&mut native);
        let status = self.invoke(&mut native)?;
        let outcome = finish_native(status, &native, self.selection)?;
        if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
            context.clear_interrupt();
        }
        drop(words);
        if let ReductionOutcome::Resume { .. } = outcome {
            environment.set(
                self.selection.index_slot,
                crate::value::Value::Number(native.index as f64),
            );
            environment.set(
                self.selection.total_slot,
                crate::value::Value::Number(native.total),
            );
        }
        Ok(Some(outcome))
    }

    fn invoke(&mut self, context: &mut NativeReductionContext) -> Result<u64, NativeDispatchError> {
        let entry = self.entry()?;
        self.physical
            .invoke(entry, |call| {
                call((context as *mut NativeReductionContext).cast())
            })
            .map_err(|error| NativeDispatchError::Physical(format!("reduction invoke: {error:?}")))
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
                |pool, address| pool.owned_array_reduction_loop_entry(address),
            )
            .map_err(|error| NativeDispatchError::Physical(format!("reduction entry: {error:?}")))
    }

    pub(crate) fn route() -> impl Iterator<Item = &'static str> {
        ["AGetI", "Add", "AddConst", "Jump", "Return"].into_iter()
    }

    pub(crate) const fn profile_name(&self) -> &'static str {
        match self.selection.profile {
            ReductionProfile::OrderedF64 => "ordered_f64_reduction_loop",
            ReductionProfile::ControlFor => "control_for_region",
            ReductionProfile::ControlWhile => "control_while_region",
            ReductionProfile::Conditional => "conditional_f64_reduction_loop",
        }
    }

    pub(crate) fn profile_route(&self) -> Vec<&'static str> {
        match self.selection.profile {
            ReductionProfile::OrderedF64 => Self::route().collect(),
            ReductionProfile::ControlFor => vec!["control", "for"],
            ReductionProfile::ControlWhile => vec!["control", "while"],
            ReductionProfile::Conditional => vec!["control", "conditional_reduction"],
        }
    }

    pub(crate) const fn region_end(&self) -> usize {
        self.selection.region_end
    }
}

impl ReductionOperation {
    fn region_key(self) -> crate::stencil_fact::RegionKey {
        match self {
            Self::Sum => crate::stencil_select::ordered_f64_reduction_loop_region_key(),
            Self::LessThan { .. } => {
                crate::stencil_select::conditional_f64_reduction_loop_region_key()
            }
        }
    }

    fn configure(self, context: &mut NativeReductionContext) {
        if let Self::LessThan {
            threshold,
            on_true,
            on_false,
        } = self
        {
            context.threshold = threshold;
            context.on_true = on_true;
            context.on_false = on_false;
        }
    }
}

fn finish_native(
    status: u64,
    native: &NativeReductionContext,
    selection: ReductionSelection,
) -> Result<ReductionOutcome, NativeDispatchError> {
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT && native.index < native.len {
        return Ok(ReductionOutcome::Resume {
            pc: selection.loop_header,
        });
    }
    let complete = status == crate::vm::NATIVE_DISPATCH_OK
        || (status == crate::vm::NATIVE_DISPATCH_INTERRUPT && native.index == native.len);
    if !complete || native.index != native.len {
        return Err(NativeDispatchError::committed(
            selection.loop_backedge,
            "ordered reduction returned incomplete progress",
        ));
    }
    Ok(ReductionOutcome::Completed(native.total))
}

fn exact_bound(value: f64) -> Option<usize> {
    let integer = value as i32;
    (integer >= 0 && f64::from(integer) == value).then_some(integer as usize)
}

fn region_image(
    view: crate::stencil_select::PhysicalStencilView,
) -> crate::stencil_region_layout::VerifiedRegionImage {
    let identity = crate::stencil_region_layout::RegionImageIdentity {
        key: view.key,
        cache_signature: fingerprint(view.stencil.bytes),
        abi: view.abi,
    };
    crate::stencil_region_layout::VerifiedRegionImage::from_composed(
        identity,
        view.stencil.bytes.to_vec(),
    )
}

fn fingerprint(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x1000_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}
