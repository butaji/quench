//! The stencil machine's implementations of the call-side staged traits (`staged::wasm_call`,
//! `staged::wasm_roots`): the indirect target's table, the native roots, and the call protocol
//! (a compiled entry, the VM's callouts, the allocating step), over the frame and context words.
//! The primitives delegate to `ConcreteWasm`; every check, trap and order stays in the rules.
//! A stage's `Exit` is `Leave`, which carries what a stencil must leave with: a trap, a callee's
//! nonzero status, or the indirect target's deferral to the VM. Not a rows file.

use crate::staged::machine::{Machine, MachineStage};
use crate::staged::wasm_call::{WasmCall, WasmTable};
use crate::staged::wasm_concrete::{ConcreteWasm, scalar_operators};
use crate::staged::wasm_roots::WasmRoots;
use crate::wasm::WasmTrap;
use crate::wasm_layout::{TARGET_BYTES, TARGET_SIGNATURE_AT};
use crate::words::{Out, S};

use super::wasm_frame::trap;

/// The canonical null signature's word (`CanonicalSignature::NULL`). A stencil's indirect target
/// never reaches the null check: `unless_equal` defers every other id to the VM.
pub(crate) const NULL_SIGNATURE: u32 = u32::MAX;

/// How a rule leaves a stencil: a trap, a nonzero status of a callee, or the deferral of an
/// indirect target to the VM's callout.
pub(crate) enum Leave {
    Trap(WasmTrap),
    Status(u64),
    Deferred,
}

/// The compiled entry of a Wasm function (`wasm_rows.rs`'s `WasmEntry`).
type Entry = unsafe extern "C" fn(usize, u64, u64, *const u64, *mut u64) -> u32;
/// The VM's defined-function callout (`wasm_call_vm.rs`): (context, function index, window, result) -> status.
type Defined = unsafe extern "C" fn(usize, u64, *const u64, *mut u64) -> i64;
/// The VM's reference callout: (context, table, index, type index, site, window, result) -> status.
type Reference = unsafe extern "C" fn(usize, u64, u64, u64, u64, *const u64, *mut u64) -> i64;
/// The table callout: (context, request, a, b, c, value, out) -> status.
type TableOp = unsafe extern "C" fn(usize, u64, u64, u64, u64, u64, *mut u64) -> i64;
/// The allocating step's callout: (context, packed, meta, frame, descriptor, out) -> status.
type Allocate = unsafe extern "C" fn(usize, u64, u64, usize, usize, *mut u64) -> i64;

/// The indirect call's table: the base and length of its element rows (`TableWords`).
pub(crate) struct TableStage {
    pub(crate) base: usize,
    pub(crate) length: u64,
}

/// The native roots of a frame: the `NativeRoots` header at `header`, and the byte offsets of its
/// `depth`, `capacity` and `slots` words.
pub(crate) struct RootsStage {
    pub(crate) header: *mut u8,
    pub(crate) depth_at: usize,
    pub(crate) capacity_at: usize,
    pub(crate) slots_at: usize,
}

/// The call protocol of a stitched frame: the frame's state, whose pins a call reloads.
pub(crate) struct CallStage<'a> {
    pub(crate) s: &'a mut S,
}

macro_rules! call_stage {
    ($($stage:ty),+) => {$(
        impl Machine for $stage {
            type Word = u64;
        }

        crate::projection!($stage => StencilStage);

        impl MachineStage for $stage {
            type Exit = Leave;

            scalar_operators! {}

            #[inline(always)]
            fn trap_when(&mut self, flag: u64, trap: WasmTrap) -> Result<(), Leave> {
                ConcreteWasm.trap_when(flag, trap).map_err(Leave::Trap)
            }
        }
    )+};
}

call_stage!(TableStage, RootsStage, CallStage<'_>);

impl TableStage {
    /// The address of element `index`'s row, which the rule has checked is inside the table.
    #[inline(always)]
    fn row(&self, index: u64) -> usize {
        self.base.wrapping_add((index as usize).wrapping_mul(TARGET_BYTES))
    }
}

impl WasmTable for TableStage {
    #[inline(always)]
    fn table_range_exceeds(&mut self, index: u64, extent: u64) -> u64 {
        u64::from(index.wrapping_add(extent) > self.length)
    }

    #[inline(always)]
    fn element_signature(&mut self, index: u64) -> u64 {
        u64::from(unsafe { *((self.row(index) + TARGET_SIGNATURE_AT) as *const u32) })
    }

    #[inline(always)]
    fn table_element(&mut self, index: u64) -> u64 {
        self.row(index) as u64
    }

    /// The row word is the element row's address, which the fast path reads its body and instance from.
    type Row = u64;

    #[inline(always)]
    fn element_row(&mut self, index: u64) -> u64 {
        self.row(index) as u64
    }

    /// Never reached: `unless_equal` defers every other signature to the VM, which relates it.
    #[inline(always)]
    fn signature_accepts(&mut self, _reference: u64, _signature: u64, _expected: u64) -> u64 {
        0
    }

    /// The expected id continues with no other check; any other id leaves for the VM's callout,
    /// which resolves the element, checks it (null, uninitialized, related) and calls it.
    #[inline(always)]
    fn unless_equal(
        &mut self,
        signature: u64,
        expected: u64,
        _otherwise: impl FnOnce(&mut Self) -> Result<u64, Leave>,
    ) -> Result<u64, Leave> {
        if signature == expected { Ok(1) } else { Err(Leave::Deferred) }
    }
}

impl WasmRoots for RootsStage {
    /// The `NativeRoots` header, which the stage holds.
    type Roots = *mut u8;

    #[inline(always)]
    fn native_roots(&mut self) -> *mut u8 {
        self.header
    }

    #[inline(always)]
    fn root_depth(&mut self, header: *mut u8) -> u64 {
        unsafe { *(header.wrapping_add(self.depth_at) as *const usize) as u64 }
    }

    #[inline(always)]
    fn root_capacity(&mut self, header: *mut u8) -> u64 {
        unsafe { *(header.wrapping_add(self.capacity_at) as *const usize) as u64 }
    }

    #[inline(always)]
    fn store_root(&mut self, header: *mut u8, depth: u64, index: usize, value: u64) {
        unsafe {
            let slots = *(header.wrapping_add(self.slots_at) as *const *mut u64);
            *slots.add((depth as usize).wrapping_add(index)) = value;
        }
    }

    #[inline(always)]
    fn set_root_depth(&mut self, header: *mut u8, depth: u64) {
        unsafe { *(header.wrapping_add(self.depth_at) as *mut usize) = depth as usize }
    }
}

impl WasmCall for CallStage<'_> {
    #[inline(always)]
    fn enter(&mut self, entry: usize, window: usize, destination: usize) -> u64 {
        let (context, base, length) = (self.s.ctx, self.s.p[1] as u64, self.s.p[2] as u64);
        unsafe {
            let entry: Entry = core::mem::transmute(entry);
            u64::from(entry(context, base, length, window as *const u64, destination as *mut u64))
        }
    }

    #[inline(always)]
    fn call_defined(&mut self, callout: usize, index: u64, window: usize, destination: usize) -> u64 {
        let context = self.s.ctx;
        unsafe {
            let callout: Defined = core::mem::transmute(callout);
            callout(context, index, window as *const u64, destination as *mut u64) as u64
        }
    }

    #[inline(always)]
    fn call_reference(
        &mut self,
        callout: usize,
        (table, index, signature, site): (u64, u64, u64, u64),
        window: usize,
        destination: usize,
    ) -> u64 {
        let context = self.s.ctx;
        unsafe {
            let callout: Reference = core::mem::transmute(callout);
            callout(context, table, index, signature, site, window as *const u64, destination as *mut u64) as u64
        }
    }

    #[inline(always)]
    fn table_callout(&mut self, callout: usize, request: u64, operands: [u64; 4], out: usize) -> u64 {
        let context = self.s.ctx;
        let [a, b, c, value] = operands;
        unsafe {
            let callout: TableOp = core::mem::transmute(callout);
            callout(context, request, a, b, c, value, out as *mut u64) as u64
        }
    }

    #[inline(always)]
    fn allocate(&mut self, callout: usize, packed: u64, meta: u64, frame: usize, descriptor: usize, out: usize) -> u64 {
        let context = self.s.ctx;
        unsafe {
            let callout: Allocate = core::mem::transmute(callout);
            callout(context, packed, meta, frame, descriptor, out as *mut u64) as u64
        }
    }

    #[inline(always)]
    fn reload_memory(&mut self, length_at: usize) {
        unsafe { super::wasm_frame::reload_memory(self.s, length_at) }
    }

    #[inline(always)]
    fn restore_memory(&mut self) {
        unsafe { super::wasm_frame::restore_memory(self.s) }
    }

    #[inline(always)]
    fn status_leaves(&mut self, status: u64) -> Result<(), Leave> {
        if status == 0 { Ok(()) } else { Err(Leave::Status(status)) }
    }

    #[inline(always)]
    fn read_result(&mut self, at: usize) -> u64 {
        unsafe { *(at as *const u64) }
    }

    #[inline(always)]
    fn write_result(&mut self, at: usize, word: u64) {
        unsafe { *(at as *mut u64) = word }
    }
}

/// The row's exit for a rule's result: continues on success, else leaves with the trap's status or
/// the callee's nonzero status. A deferral is the indirect row's own, answered before any call rule
/// runs, so it leaves with status 0 here.
#[inline(always)]
pub(crate) fn leave_with(mut s: S, result: Result<(), Leave>) -> Out {
    match result {
        Ok(()) => Out::Next(s),
        Err(Leave::Trap(error)) => trap(s, error),
        Err(Leave::Status(status)) => {
            s.p[0] = status as usize;
            Out::Exit(s)
        }
        Err(Leave::Deferred) => {
            s.p[0] = 0;
            Out::Exit(s)
        }
    }
}
