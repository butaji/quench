//! The one World a Wasm function's rows stitch under (`wasm_stitch::stitch`): what the function's plan
//! gives every row (its globals, table bindings, held references, GC reads, vectors), with what the live
//! program and an image name differently (`Naming`): callees, indirect sites, callouts, whether the
//! memory keeps its base, and which constants a row may hold.
//!
//! Owner: base tier (Wasm rows). Both the b-jit's instance compile (`stitched_entry`) and the image
//! driver (`wasm_image::rows`) build their World here.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The callouts a function's rows call, by address.
pub(crate) struct Callouts {
    pub(crate) vm_call: u64,
    pub(crate) table: u64,
    pub(crate) gc: u64,
    pub(crate) allocate: u64,
}

impl Callouts {
    /// This process's callouts: the live program's rows call them in place.
    pub(crate) fn live() -> Self {
        Self {
            vm_call: wasm_call_vm::vm_call_callout as *const () as u64,
            table: wasm_table_ops_stage::table_op_callout as *const () as u64,
            gc: wasm_gc_callout::wasm_gc_callout as *const () as u64,
            allocate: wasm_gc_allocate::allocate_frame as *const () as u64,
        }
    }

    /// One value for every callout.
    pub(crate) const fn all(word: u64) -> Self {
        Self { vm_call: word, table: word, gc: word, allocate: word }
    }
}

/// What the live program and an image name differently in a function's rows.
pub(crate) struct Naming<'n> {
    /// Whether a constant's frame word may sit in a row.
    pub(crate) constant: &'n dyn Fn(u64) -> bool,
    /// The word a direct call of a callee calls through (`World::callee`).
    pub(crate) callee: &'n dyn Fn(usize) -> Option<u64>,
    /// The indirect site of a `Call::Reference` at a pc (`World::indirect`).
    pub(crate) indirect: &'n dyn Fn(usize) -> Option<wasm_stitch::IndirectSite>,
    /// Whether the memory keeps its base when it grows (`World::memory_stable`).
    pub(crate) memory_stable: bool,
    pub(crate) callouts: Callouts,
}

impl Analysis<'_> {
    /// Runs `stitch` with the World of `function`, whose full plan is `full_plan`, under `naming`.
    pub(crate) fn with_world<R>(
        &self,
        function: &quench_runtime::bytecode::Function,
        full_plan: &Plan,
        memory_slot: Option<u32>,
        naming: Naming<'_>,
        stitch: impl FnOnce(&wasm_stitch::World<'_>) -> R,
    ) -> R {
        let (vm, program) = (self.vm, self.program);
        let constant = |at: usize| {
            let value = vm.programs.constant(program, at)?;
            if vm.heap_reads().v128(value).is_some() {
                return None;
            }
            let bits = vm.decode_bits64(value).unwrap_or(u64::from(value.wasm_bits32()));
            (naming.constant)(bits).then_some(bits)
        };
        // A global access names its environment slot in the full plan; the row reads the cell from
        // the context, since a program's code serves every instance.
        let global = |pc: usize| -> Option<(u32, quench_runtime::staged::wasm_instance::Storage)> {
            if !full_plan.reachable(pc) {
                return None;
            }
            let Slot::Binding(slot) = full_plan.operand(pc) else { return None };
            Some((slot, quench_runtime::staged::wasm_instance::Storage::of(self.global_type(slot)?)))
        };
        let vector_constant = |at: usize| vm.heap_reads().v128(vm.programs.constant(program, at)?);
        let vector_move = |pc: usize| full_plan.reachable(pc) && full_plan.operand(pc) == Slot::Vector;
        // A table or segment operation names its bindings in the full plan, as the u-jit reads them.
        let table_bindings = |pc: usize| -> Option<(Option<u32>, Option<u32>)> {
            if !full_plan.reachable(pc) {
                return None;
            }
            let first = match full_plan.operand(pc) {
                Slot::Binding(slot) => Some(slot),
                _ => None,
            };
            let second = match full_plan.seconds[pc] {
                Some(Slot::Binding(slot)) => Some(slot),
                _ => None,
            };
            Some((first, second))
        };
        // A call holds the reference registers its state names (`wasm_roots`), and an allocating step
        // reads the operands its plan gives their kinds (`wasm_gc_allocate`).
        let references = |pc: usize| -> Vec<u16> { full_plan.references.get(pc).cloned().unwrap_or_default() };
        let gc_reads = |pc: usize| -> Vec<(u16, u64)> {
            full_plan.gc_reads.get(pc).map_or_else(Vec::new, |reads| reads.iter().map(|(register, kind)| (*register, *kind as u64)).collect())
        };
        let world = wasm_stitch::World {
            constant: &constant,
            memory_slot,
            memory_stable: naming.memory_stable,
            callee: naming.callee,
            grows_memory: &self.grows_memory,
            vm_call: naming.callouts.vm_call,
            global: &global,
            globals_offset: CONTEXT_GLOBALS as u64,
            indirect: naming.indirect,
            table_callout: naming.callouts.table,
            gc_callout: naming.callouts.gc,
            allocate_callout: naming.callouts.allocate,
            references: &references,
            gc_reads: &gc_reads,
            table_bindings: &table_bindings,
            registers: function.registers,
            vector_constant: &vector_constant,
            vector_move: &vector_move,
        };
        stitch(&world)
    }
}
