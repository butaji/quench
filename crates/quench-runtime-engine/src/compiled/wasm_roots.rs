//! The reference registers of a compiled frame, held in the VM's native roots across the calls
//! that can collect. A collection runs only at an interpreter safepoint, so a call that re-enters
//! the VM or runs another function (a known or indirect call, a VM callout, a table or memory
//! call-out that may run JS) can free what the frame holds in its reference registers unless the
//! collector sees them. The frame pushes each reference register it holds before such a call and
//! pops them after it returns, before the call's status propagates. A call-out that only allocates
//! or reads cells runs no interpreter code, so it holds nothing.
//!
//! The registers pushed are those the analysis states hold as references at the instruction
//! (`Plan::references`): a register that is a reference on every path to the call, whose `I64`
//! view holds its reference bits. A register that is a reference on one path and a number on
//! another is not among them, so no number's bits are pushed as a Value.
//!
//! The push and the restore are the staged rules (`staged::wasm_roots`), which the stencil runs
//! over its own roots too: this file is the `WasmRoots` of the Cranelift emitter.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::instance_context::NativeRoots;
use super::wasm_stage::infallible;
use super::{CONTEXT_NATIVE_ROOTS, Emitter, Kind, invariant};
use quench_runtime::staged::machine::{MachineAccess, MachineStage};
use quench_runtime::staged::wasm::Width;
use quench_runtime::staged::wasm_roots::{WasmRoots, push, restore};
use quench_runtime::wasm::integer::I64BinaryOperator as Binary;
use cranelift_codegen::ir::{self, MemFlagsData};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The bytes of one native root.
const ROOT: usize = size_of::<quench_runtime::Value>();

impl Emitter<'_, '_> {
    /// Holds the reference registers the instruction being emitted holds (`Emitter::roots`) in the
    /// native roots, and returns the depth before them for `unroot`. Where the roots have no room
    /// for them, the call traps `CallStackExhausted`, as the native stack's budget would. An
    /// instruction that holds no reference emits nothing and returns `None`.
    pub(crate) fn root(&mut self) -> Option<ir::Value> {
        let registers = self.roots;
        if registers.is_empty() {
            return None;
        }
        // A reference's Wasm bits are its Value bits, the `I64` view of the register.
        let values: Vec<ir::Value> = registers.iter().map(|register| self.read(*register, Kind::Reference)).collect();
        Some(infallible(push(self, &values)))
    }

    /// Releases the roots `root` held: the depth it returned is restored. Emitted before the
    /// call's status propagates, so an error leaves with the depth it found.
    pub(crate) fn unroot(&mut self, depth: Option<ir::Value>) {
        let Some(depth) = depth else {
            return;
        };
        restore(self, depth);
    }
}

impl WasmRoots for Emitter<'_, '_> {
    /// The roots' address, a value in the function: the rule loads it once and every access reuses it.
    type Roots = ir::Value;

    fn native_roots(&mut self) -> ir::Value {
        let context = self.body.core.context;
        self.load_word(Width::I64, invariant(), context, CONTEXT_NATIVE_ROOTS)
    }

    fn root_depth(&mut self, roots: ir::Value) -> ir::Value {
        self.load_word(Width::I64, MemFlagsData::trusted(), roots, NativeRoots::DEPTH)
    }

    fn root_capacity(&mut self, roots: ir::Value) -> ir::Value {
        self.load_word(Width::I64, MemFlagsData::trusted(), roots, NativeRoots::CAPACITY)
    }

    fn store_root(&mut self, roots: ir::Value, depth: ir::Value, index: usize, value: ir::Value) {
        let flags = MemFlagsData::trusted();
        let slots = self.load_word(Width::I64, flags, roots, NativeRoots::SLOTS);
        let size = self.constant(Width::I64, ROOT as i64);
        let offset = self.operate(Binary::Multiply, Width::I64, depth, size);
        let top = self.operate(Binary::Add, Width::I64, slots, offset);
        self.store_word(flags, value, top, (index * ROOT) as i32);
    }

    fn set_root_depth(&mut self, roots: ir::Value, depth: ir::Value) {
        self.store_word(MemFlagsData::trusted(), depth, roots, NativeRoots::DEPTH);
    }
}
