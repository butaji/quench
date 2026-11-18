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

use super::instance_context::NativeRoots;
use super::{CONTEXT_NATIVE_ROOTS, Emitter, Host, Kind, invariant};
use crate::WasmTrap;
use crate::staged::machine::{MachineAccess, MachineStage};
use crate::staged::wasm::Width;
use crate::wasm::integer::I64BinaryOperator as Binary;
use cranelift_codegen::ir::{self, MemFlagsData};

/// The bytes of one native root.
const ROOT: usize = size_of::<crate::Value>();

impl<H: Host> Emitter<'_, '_, H> {
    /// Holds the reference registers the instruction being emitted holds (`Emitter::roots`) in the
    /// native roots, and returns the depth before them for `unroot`. Where the roots have no room
    /// for them, the call traps `CallStackExhausted`, as the native stack's budget would. An
    /// instruction that holds no reference emits nothing and returns `None`.
    pub(super) fn root(&mut self) -> Option<ir::Value> {
        let registers = self.roots;
        if registers.is_empty() {
            return None;
        }
        let flags = MemFlagsData::trusted();
        let context = self.body.core.context;
        let roots = self.load_word(Width::I64, invariant(), context, CONTEXT_NATIVE_ROOTS);
        let depth = self.load_word(Width::I64, flags, roots, NativeRoots::DEPTH);
        let count = self.constant(Width::I64, registers.len() as i64);
        let needed = self.operate(Binary::Add, Width::I64, depth, count);
        let capacity = self.load_word(Width::I64, flags, roots, NativeRoots::CAPACITY);
        let exhausted = self.operate(Binary::LessUnsigned, Width::I64, capacity, needed);
        self.trap_if(exhausted, WasmTrap::CallStackExhausted);
        let slots = self.load_word(Width::I64, flags, roots, NativeRoots::SLOTS);
        let size = self.constant(Width::I64, ROOT as i64);
        let offset = self.operate(Binary::Multiply, Width::I64, depth, size);
        let top = self.operate(Binary::Add, Width::I64, slots, offset);
        for (index, register) in registers.iter().enumerate() {
            // A reference's Wasm bits are its Value bits, the `I64` view of the register.
            let value = self.read(*register, Kind::Reference);
            self.store_word(flags, value, top, (index * ROOT) as i32);
        }
        self.store_word(flags, needed, roots, NativeRoots::DEPTH);
        Some(depth)
    }

    /// Releases the roots `root` held: the depth it returned is restored. Emitted before the
    /// call's status propagates, so an error leaves with the depth it found.
    pub(super) fn unroot(&mut self, depth: Option<ir::Value>) {
        let Some(depth) = depth else {
            return;
        };
        let context = self.body.core.context;
        let roots = self.load_word(Width::I64, invariant(), context, CONTEXT_NATIVE_ROOTS);
        self.store_word(MemFlagsData::trusted(), depth, roots, NativeRoots::DEPTH);
    }
}
