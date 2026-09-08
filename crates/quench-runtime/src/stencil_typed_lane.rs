//! Guarded Int32 lane transforms with an ordered Number reduction.

use std::{cell::RefCell, rc::Rc};

const MACHINE_SLAB_BYTES: usize = 4096;
const MAX_ITERATIONS: usize = 1 << 20;
const BYTES_PER_LANE: usize = std::mem::size_of::<i32>();

#[derive(Clone, Copy, Debug)]
struct TypedLane {
    counted: crate::stencil_counted_loop::CountedLoop,
    total_slot: u16,
    array_slot: u16,
    xor_mask: i32,
    adjustment: i32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FunctionTypedLane {
    selected: TypedLane,
    initial_total: f64,
}

#[path = "stencil_typed_lane_select.rs"]
mod selection;

pub(crate) use selection::select_function;

impl FunctionTypedLane {
    pub(crate) fn execute_native(
        self,
        function: &crate::value::FunctionValue,
        arguments: &[crate::value::Value],
    ) -> Result<Option<f64>, crate::execute::VmError> {
        let Some(array) =
            input_value(function, arguments, self.selected.array_slot).and_then(int32_view)
        else {
            return Ok(None);
        };
        execute_with_view(self, &array)
    }
}

fn input_value(
    function: &crate::value::FunctionValue,
    arguments: &[crate::value::Value],
    slot: u16,
) -> Option<crate::value::Value> {
    let captures = u16::try_from(function.captures.len()).ok()?;
    if slot < captures {
        return Some(function.captures.get(slot));
    }
    arguments.get(usize::from(slot - captures)).cloned()
}

fn int32_view(value: crate::value::Value) -> Option<Rc<crate::value::Int32ArrayData>> {
    let crate::value::Value::Int32Array(view) = value else {
        return None;
    };
    Some(view)
}

fn execute_with_view(
    fact: FunctionTypedLane,
    view: &Rc<crate::value::Int32ArrayData>,
) -> Result<Option<f64>, crate::execute::VmError> {
    let Some(end) = validate_view(fact, view) else {
        return Ok(None);
    };
    let mut bytes = match view.buffer.bytes.try_borrow_mut() {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    let Some(pointer) = lane_pointer(&mut bytes, view.byte_offset, end) else {
        return Ok(None);
    };
    let context = TypedLaneContext::new(fact, pointer);
    TYPED_LANE_MACHINE.with(|machine| execute_machine(machine, context))
}

fn validate_view(
    fact: FunctionTypedLane,
    view: &Rc<crate::value::Int32ArrayData>,
) -> Option<usize> {
    let loop_ = fact.selected.counted;
    let iterations = usize::try_from(loop_.end.checked_sub(loop_.start)?).ok()?;
    let end = usize::try_from(loop_.end).ok()?;
    (loop_.start >= 0 && iterations <= MAX_ITERATIONS && view.logical_len() >= end).then_some(())?;
    (!view.buffer.shared && !view.buffer.immutable && view.buffer.max_byte_length.is_none())
        .then_some(())?;
    let value = crate::value::Value::Int32Array(Rc::clone(view));
    (!crate::arrays::typed_array_is_detached(&value)
        && !crate::typed_array_prototype::is_out_of_bounds(&value))
    .then_some(end)
}

fn lane_pointer(bytes: &mut [u8], offset: usize, lanes: usize) -> Option<*mut i32> {
    (offset % BYTES_PER_LANE == 0).then_some(())?;
    let length = lanes.checked_mul(BYTES_PER_LANE)?;
    let end = offset.checked_add(length)?;
    let slice = bytes.get_mut(offset..end)?;
    Some(slice.as_mut_ptr().cast())
}

#[repr(C)]
struct TypedLaneContext {
    values: *mut i32,
    index: u32,
    end: u32,
    xor_mask: i32,
    adjustment: i32,
    total: f64,
    interrupt: *const std::sync::atomic::AtomicBool,
}

impl TypedLaneContext {
    fn new(fact: FunctionTypedLane, values: *mut i32) -> Self {
        Self {
            values,
            index: fact.selected.counted.start as u32,
            end: fact.selected.counted.end as u32,
            xor_mask: fact.selected.xor_mask,
            adjustment: fact.selected.adjustment,
            total: fact.initial_total,
            interrupt: crate::vm::current_context_or_default().interrupt_flag(),
        }
    }
}

struct TypedLaneMachine {
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>>,
}

thread_local! {
    static TYPED_LANE_MACHINE: RefCell<Option<TypedLaneMachine>> = const { RefCell::new(None) };
}

impl TypedLaneMachine {
    fn new() -> Option<Self> {
        let key = crate::stencil_select::typed_lane_loop_region_key();
        let abi = crate::stencil_select::RegionAbi::TypedLaneLoop;
        let view = crate::stencil_select::select_physical_for_abi(key, abi)?;
        (view.generated && view.executable && view.stencil.validate()).then_some(())?;
        crate::machine::validate_physical_view(view.record, view.stencil).ok()?;
        let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::ASetI);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let image = crate::stencil_region_layout::finalize_selected_leaf(view, &values).ok()?;
        Some(Self {
            owner: Rc::new(RefCell::new(
                crate::stencil_arena::SharedStencilSlab::new(MACHINE_SLAB_BYTES).ok()?,
            )),
            image,
            cache: crate::stencil_select::RenderedRegionCache::new(),
            installed: None,
        })
    }

    fn invoke(&mut self, context: &mut TypedLaneContext) -> Option<u64> {
        let entry = self.entry()?;
        let lease =
            crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry).ok()?;
        lease
            .invoke(|call| call((context as *mut TypedLaneContext).cast()))
            .ok()
    }

    fn entry(
        &mut self,
    ) -> Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>> {
        if let Some(entry) = self
            .installed
            .filter(|entry| self.owner.borrow().entry_token_is_live(*entry))
        {
            return Some(entry);
        }
        let address = self
            .owner
            .borrow_mut()
            .publish_region_image_or_get(&mut self.cache, &self.image)
            .ok()?;
        let entry = self
            .owner
            .borrow()
            .owned_typed_lane_loop_entry(address)
            .ok()?;
        self.installed = Some(entry);
        Some(entry)
    }
}

fn execute_machine(
    machine: &RefCell<Option<TypedLaneMachine>>,
    mut context: TypedLaneContext,
) -> Result<Option<f64>, crate::execute::VmError> {
    let mut machine = machine.borrow_mut();
    if machine.is_none() {
        *machine = TypedLaneMachine::new();
    }
    let Some(status) = machine
        .as_mut()
        .and_then(|machine| machine.invoke(&mut context))
    else {
        return Ok(None);
    };
    drop(machine);
    finish_status(status, &mut context)
}

fn finish_status(
    status: u64,
    context: &mut TypedLaneContext,
) -> Result<Option<f64>, crate::execute::VmError> {
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
        crate::vm::current_context_or_default().clear_interrupt();
        finish_portable(context);
    }
    if matches!(
        status,
        crate::vm::NATIVE_DISPATCH_OK | crate::vm::NATIVE_DISPATCH_INTERRUPT
    ) {
        return Ok(Some(context.total));
    }
    Err(crate::execute::VmError::EvalError(
        "typed lane region returned an invalid post-entry status".into(),
    ))
}

fn finish_portable(context: &mut TypedLaneContext) {
    while context.index < context.end {
        portable_iteration(context);
    }
}

fn portable_iteration(context: &mut TypedLaneContext) {
    let lane = ((context.index as i32) ^ context.xor_mask).wrapping_add(context.adjustment);
    // SAFETY: admission retains the exclusive backing borrow and bounds the index.
    unsafe { context.values.add(context.index as usize).write(lane) };
    let number = f64::from(lane);
    context.total += number * number;
    context.index += 1;
}
