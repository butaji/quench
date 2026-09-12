//! Guarded counted-I32 regions selected from shared value and loop facts.

use std::{cell::RefCell, rc::Rc};

const MACHINE_SLAB_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResultRepresentation {
    I32,
    U32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CountedI32Recurrence {
    counted: crate::stencil_counted_loop::CountedLoop,
    initial: i32,
    shift: u32,
    multiplier: i32,
    result: ResultRepresentation,
}

#[path = "stencil_counted_i32_recurrence_select.rs"]
mod selection;

pub(crate) use selection::select_function;

impl CountedI32Recurrence {
    pub(crate) fn execute_native(
        self,
        function: &crate::value::FunctionValue,
    ) -> Result<Option<f64>, crate::execute::VmError> {
        if !intrinsic_guard(function) || !self.range_is_bounded() {
            return Ok(None);
        }
        let mut context = CountedI32Context::new(self);
        let Some(status) = COUNTED_I32_MACHINE.with(|machine| invoke(machine, &mut context)) else {
            return Ok(None);
        };
        finish(status, &mut context)?;
        Ok(Some(self.result_value(context.value)))
    }

    fn range_is_bounded(self) -> bool {
        self.counted.start >= 0 && self.counted.end.checked_sub(self.counted.start).is_some()
    }

    fn result_value(self, value: i32) -> f64 {
        match self.result {
            ResultRepresentation::I32 => f64::from(value),
            ResultRepresentation::U32 => f64::from(value as u32),
        }
    }
}

#[repr(C)]
struct CountedI32Context {
    index: i32,
    end: i32,
    value: i32,
    shift: u32,
    multiplier: i32,
    interrupt: *const std::sync::atomic::AtomicBool,
}

impl CountedI32Context {
    fn new(selected: CountedI32Recurrence) -> Self {
        Self {
            index: selected.counted.start,
            end: selected.counted.end,
            value: selected.initial,
            shift: selected.shift,
            multiplier: selected.multiplier,
            interrupt: crate::vm::current_context_or_default().interrupt_flag(),
        }
    }
}

struct CountedI32Machine {
    image: crate::stencil_region_layout::VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<crate::stencil_arena::DispatchEntry>,
}

thread_local! {
    static COUNTED_I32_MACHINE: RefCell<Option<CountedI32Machine>> = const { RefCell::new(None) };
}

impl CountedI32Machine {
    fn new() -> Option<Self> {
        let key = crate::stencil_select::counted_i32_recurrence_region_key();
        let abi = crate::stencil_select::RegionAbi::NumericI32BitwiseLoop;
        let view = crate::stencil_select::select_physical_for_abi(key, abi)?;
        (view.generated && view.executable && view.stencil.validate()).then_some(())?;
        crate::machine::validate_physical_view(view.record, view.stencil).ok()?;
        let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::Binary);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let image = crate::stencil_region_layout::finalize_selected_leaf(view, &values).ok()?;
        Some(Self {
            physical: crate::stencil_installation::SharedPhysicalEntry::new(Rc::new(RefCell::new(
                crate::stencil_arena::SharedStencilSlab::new(MACHINE_SLAB_BYTES).ok()?,
            ))),
            image,
        })
    }

    fn invoke(&mut self, context: &mut CountedI32Context) -> Option<u64> {
        let entry = self.entry()?;
        self.physical
            .invoke(entry, |call| {
                call((context as *mut CountedI32Context).cast())
            })
            .ok()
    }

    fn entry(
        &mut self,
    ) -> Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>> {
        let image = &self.image;
        self.physical
            .entry(
                |owner, cache| owner.borrow_mut().publish_region_image_or_get(cache, image),
                |pool, address| pool.owned_numeric_i32_bitwise_loop_entry(address),
            )
            .ok()
    }
}

fn invoke(
    machine: &RefCell<Option<CountedI32Machine>>,
    context: &mut CountedI32Context,
) -> Option<u64> {
    let mut machine = machine.borrow_mut();
    if machine.is_none() {
        *machine = CountedI32Machine::new();
    }
    machine.as_mut()?.invoke(context)
}

fn finish(status: u64, context: &mut CountedI32Context) -> Result<(), crate::execute::VmError> {
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
        crate::vm::current_context_or_default().clear_interrupt();
        return finish_portable(context);
    }
    if status == crate::vm::NATIVE_DISPATCH_OK && context.index == context.end {
        return Ok(());
    }
    Err(crate::execute::VmError::EvalError(
        "counted I32 region returned an invalid post-entry status".into(),
    ))
}

fn finish_portable(context: &mut CountedI32Context) -> Result<(), crate::execute::VmError> {
    while context.index < context.end {
        let shifted = (context.value as u32) >> (context.shift & 31);
        context.value = (context.value ^ shifted as i32).wrapping_mul(context.multiplier);
        context.index += 1;
    }
    Ok(())
}

fn intrinsic_guard(function: &crate::value::FunctionValue) -> bool {
    function
        .captures
        .with_proven_object(0, intrinsic_global_guard)
        .unwrap_or(false)
}

fn intrinsic_global_guard(global: &crate::value::ObjectData) -> bool {
    let intrinsic = crate::vm::proven_own_word(global, "Math")
        .is_some_and(|word| word.is_builtin(crate::ops::Builtin::Math));
    let clean_override =
        crate::builtins::read_intrinsic_override(crate::ops::Builtin::Math, "imul").is_none();
    let present =
        !crate::builtins::builtin_prototype_property_is_removed(crate::ops::Builtin::Math, "imul");
    intrinsic && clean_override && present
}
