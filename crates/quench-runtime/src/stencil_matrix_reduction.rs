//! Guarded rectangular dense-matrix reductions over ordinary nested loops.

use std::{cell::RefCell, rc::Rc};

const MACHINE_SLAB_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug)]
struct MatrixReduction {
    loops: [crate::stencil_counted_loop::CountedLoop; 3],
    total_slot: u16,
    left_slot: u16,
    right_slot: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FunctionMatrixReduction {
    reduction: MatrixReduction,
    initial_total: f64,
}

#[path = "stencil_matrix_reduction_select.rs"]
mod selection;

pub(crate) use selection::select_function;

impl FunctionMatrixReduction {
    pub(crate) fn execute_native(
        self,
        function: &crate::value::FunctionValue,
        arguments: &[crate::value::Value],
    ) -> Result<Option<f64>, crate::execute::VmError> {
        let Some(()) = validate_loop_bounds(self.reduction.loops) else {
            return Ok(None);
        };
        let Some(left) =
            input_value(function, arguments, self.reduction.left_slot).and_then(numeric_rows)
        else {
            return Ok(None);
        };
        let Some(right) =
            input_value(function, arguments, self.reduction.right_slot).and_then(numeric_rows)
        else {
            return Ok(None);
        };
        execute_with_rows(self, &left, &right)
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

fn numeric_rows(value: crate::value::Value) -> Option<Vec<Rc<crate::value::ArrayData>>> {
    let crate::value::Value::Array(matrix) = value else {
        return None;
    };
    (crate::locals::array_word_is_current(&matrix) && matrix.is_plain_dense_access())
        .then_some(())?;
    matrix
        .packed_values()?
        .into_iter()
        .map(|row| match row {
            crate::value::Value::Array(row)
                if crate::locals::array_word_is_current(&row) && row.is_dense_numeric_data() =>
            {
                Some(row)
            }
            _ => None,
        })
        .collect()
}

fn execute_with_rows(
    fact: FunctionMatrixReduction,
    left: &[Rc<crate::value::ArrayData>],
    right: &[Rc<crate::value::ArrayData>],
) -> Result<Option<f64>, crate::execute::VmError> {
    let Some(dimensions) = dimensions(fact.reduction.loops) else {
        return Ok(None);
    };
    let Some(()) = validate_rows(left, right, dimensions) else {
        return Ok(None);
    };
    let Some(left_words) = borrow_rows(left) else {
        return Ok(None);
    };
    let Some(right_words) = borrow_rows(right) else {
        return Ok(None);
    };
    let left_ptrs = left_words
        .iter()
        .map(|row| row.as_ptr())
        .collect::<Vec<_>>();
    let right_ptrs = right_words
        .iter()
        .map(|row| row.as_ptr())
        .collect::<Vec<_>>();
    let context = MatrixReductionContext::new(fact, &left_ptrs, &right_ptrs);
    MATRIX_MACHINE.with(|machine| execute_machine(machine, context))
}

fn borrow_rows<'a>(
    rows: &'a [Rc<crate::value::ArrayData>],
) -> Option<Vec<std::cell::Ref<'a, [f64]>>> {
    rows.iter().map(|row| row.numeric_kernel_words()).collect()
}

fn dimensions(loops: [crate::stencil_counted_loop::CountedLoop; 3]) -> Option<[usize; 3]> {
    loops
        .map(|loop_| usize::try_from(loop_.end.checked_sub(loop_.start)?).ok())
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn validate_loop_bounds(loops: [crate::stencil_counted_loop::CountedLoop; 3]) -> Option<()> {
    let dimensions = dimensions(loops)?;
    loops.iter().all(|loop_| loop_.start == 0).then_some(())?;
    dimensions
        .into_iter()
        .try_fold(1usize, usize::checked_mul)
        .map(|_| ())
}

fn validate_rows(
    left: &[Rc<crate::value::ArrayData>],
    right: &[Rc<crate::value::ArrayData>],
    [rows, columns, inner]: [usize; 3],
) -> Option<()> {
    (left.len() >= rows && right.len() >= inner).then_some(())?;
    left.iter()
        .take(rows)
        .all(|row| row.logical_len() >= inner)
        .then_some(())?;
    right
        .iter()
        .take(inner)
        .all(|row| row.logical_len() >= columns)
        .then_some(())
}

#[repr(C)]
struct MatrixReductionContext {
    left_rows: *const *const f64,
    right_rows: *const *const f64,
    row: u32,
    rows: u32,
    column: u32,
    columns: u32,
    inner: u32,
    inner_length: u32,
    total: f64,
    interrupt: *const std::sync::atomic::AtomicBool,
}

impl MatrixReductionContext {
    fn new(fact: FunctionMatrixReduction, left: &[*const f64], right: &[*const f64]) -> Self {
        let [rows, columns, inner] = dimensions(fact.reduction.loops).unwrap_or([0; 3]);
        Self {
            left_rows: left.as_ptr(),
            right_rows: right.as_ptr(),
            row: 0,
            rows: rows as u32,
            column: 0,
            columns: columns as u32,
            inner: 0,
            inner_length: inner as u32,
            total: fact.initial_total,
            interrupt: crate::vm::current_context_or_default().interrupt_flag(),
        }
    }
}

struct MatrixMachine {
    image: crate::stencil_region_layout::VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<crate::stencil_arena::DispatchEntry>,
}

thread_local! {
    static MATRIX_MACHINE: RefCell<Option<MatrixMachine>> = const { RefCell::new(None) };
}

impl MatrixMachine {
    fn new() -> Option<Self> {
        let key = crate::stencil_select::matrix_reduction_loop_region_key();
        let abi = crate::stencil_select::RegionAbi::MatrixReductionLoop;
        let view = crate::stencil_select::select_physical_for_abi(key, abi)?;
        (view.generated && view.executable && view.stencil.validate()).then_some(())?;
        crate::machine::validate_physical_view(view.record, view.stencil).ok()?;
        let site = crate::quickening::QuickeningSite::<4>::new(crate::ir::Opcode::AGetI);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let image = crate::stencil_region_layout::finalize_selected_leaf(view, &values).ok()?;
        let owner = Rc::new(RefCell::new(
            crate::stencil_arena::SharedStencilSlab::new(MACHINE_SLAB_BYTES).ok()?,
        ));
        Some(Self {
            image,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
        })
    }

    fn invoke(&mut self, context: &mut MatrixReductionContext) -> Option<u64> {
        let entry = self.entry()?;
        self.physical
            .invoke(entry, |call| {
                call((context as *mut MatrixReductionContext).cast())
            })
            .ok()
    }

    fn entry(
        &mut self,
    ) -> Option<crate::stencil_arena::EntryToken<crate::stencil_arena::DispatchEntry>> {
        self.physical
            .entry(
                |owner, cache| {
                    owner
                        .borrow_mut()
                        .publish_region_image_or_get(cache, &self.image)
                },
                |pool, address| pool.owned_matrix_reduction_loop_entry(address),
            )
            .ok()
    }
}

fn execute_machine(
    machine: &RefCell<Option<MatrixMachine>>,
    mut context: MatrixReductionContext,
) -> Result<Option<f64>, crate::execute::VmError> {
    let mut machine = machine.borrow_mut();
    if machine.is_none() {
        *machine = MatrixMachine::new();
    }
    let Some(status) = machine
        .as_mut()
        .and_then(|machine| machine.invoke(&mut context))
    else {
        return Ok(None);
    };
    drop(machine);
    if status == crate::vm::NATIVE_DISPATCH_INTERRUPT {
        crate::vm::current_context_or_default().clear_interrupt();
        finish_portable(&mut context);
    }
    if matches!(
        status,
        crate::vm::NATIVE_DISPATCH_OK | crate::vm::NATIVE_DISPATCH_INTERRUPT
    ) {
        return Ok(Some(context.total));
    }
    Err(crate::execute::VmError::EvalError(
        "matrix reduction returned an invalid post-entry status".into(),
    ))
}

fn finish_portable(context: &mut MatrixReductionContext) {
    while context.row < context.rows {
        if context.column >= context.columns {
            context.column = 0;
            context.row += 1;
        } else if context.inner >= context.inner_length {
            context.inner = 0;
            context.column += 1;
        } else {
            portable_iteration(context);
        }
    }
}

fn portable_iteration(context: &mut MatrixReductionContext) {
    let row = context.row as usize;
    let column = context.column as usize;
    let inner = context.inner as usize;
    // SAFETY: admission retained every numeric row borrow and validated all dimensions.
    unsafe {
        let left = *(*context.left_rows.add(row)).add(inner);
        let right = *(*context.right_rows.add(inner)).add(column);
        context.total += left * right;
    }
    context.inner += 1;
}
