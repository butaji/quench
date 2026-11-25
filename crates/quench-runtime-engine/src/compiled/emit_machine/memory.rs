//! The memory family of the machine: `MachineAccess` as Cranelift loads, stores and calls. An
//! `Access` of Wasm memory is a lane, an extension and a result lane over the primitives.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{Builds, of_in_lane, machine_type};
use quench_runtime::staged::machine::{CallTarget, Extend, Lane, MachineAccess};
use quench_runtime::staged::wasm_memory::Access;
use cranelift_codegen::ir::{self, InstBuilder, MemFlagsData, types};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::FunctionBuilder;
use super::super::{Kind, scalar_ir};

/// The byte widths of the narrow accesses.
const BYTE: usize = size_of::<u8>();
const HALF: usize = size_of::<u16>();
const WORD: usize = size_of::<u32>();

/// The convention of a word call (`call_words`, `call_lanes`), whose target is Rust (an
/// `extern "C"` callout): the platform's default. A u-jit body runs under `Tail`
/// (`BODY_CALL_CONV`), which a callout does not follow: its stack arguments (a seventh word on
/// x64) are the callee's to pop under `Tail`, never under the C convention, so a callout called
/// under the body's convention would leave the stack pointer off.
fn leaf_call_conv(caller: CallConv) -> CallConv {
    match caller {
        CallConv::Tail => CallConv::triple_default(&target_lexicon::Triple::host()),
        call_conv => call_conv,
    }
}

/// The memory cell an access of `bytes` reads: an integer lane.
fn cell_lane(bytes: usize) -> Lane {
    match bytes {
        BYTE => Lane::I8,
        HALF => Lane::I16,
        WORD => Lane::I32,
        _ => Lane::I64,
    }
}

/// The `from` cell at `base + offset` extended into `to`; a 32-bit cell extends into an `i64`.
fn extended_load(
    builder: &mut FunctionBuilder<'_>,
    (from, extend, to): (Lane, Extend, Lane),
    flags: MemFlagsData,
    (base, offset): (ir::Value, i32),
) -> ir::Value {
    let ty = machine_type(to);
    let ins = builder.ins();
    match (from, extend) {
        (Lane::I8, Extend::Zero) => ins.uload8(ty, flags, base, offset),
        (Lane::I16, Extend::Zero) => ins.uload16(ty, flags, base, offset),
        (Lane::I8, Extend::Sign) => ins.sload8(ty, flags, base, offset),
        (Lane::I16, Extend::Sign) => ins.sload16(ty, flags, base, offset),
        (_, Extend::Zero) => ins.uload32(flags, base, offset),
        (_, Extend::Sign) => ins.sload32(flags, base, offset),
    }
}

crate::compiled::emit_machine::for_each_builder!(MachineAccess {
    type Flags = MemFlagsData;
    type Function = ir::FuncRef;
    type Signature = ir::SigRef;
    type Slot = ir::StackSlot;
    type Symbol = ir::GlobalValue;

    fn raw_load(&mut self, lane: Lane, flags: MemFlagsData, base: ir::Value, offset: i32) -> ir::Value {
        self.function_builder().ins().load(machine_type(lane), flags, base, offset)
    }

    fn raw_load_extended(&mut self, from: Lane, extend: Extend, to: Lane, flags: MemFlagsData, base: ir::Value, offset: i32) -> ir::Value {
        extended_load(self.function_builder(), (from, extend, to), flags, (base, offset))
    }

    fn raw_store(&mut self, lane: Lane, flags: MemFlagsData, (value, private): (ir::Value, bool), base: ir::Value, offset: i32) {
        let builder = self.function_builder();
        let wide = builder.func.dfg.value_type(value) == types::I64;
        let ins = builder.ins();
        let store = match lane {
            Lane::I8 => ins.istore8(flags, value, base, offset),
            Lane::I16 => ins.istore16(flags, value, base, offset),
            Lane::I32 if wide => ins.istore32(flags, value, base, offset),
            _ => ins.store(flags, value, base, offset),
        };
        if private { builder.func.private_stores.push(store); }
    }

    fn raw_atomic_load(&mut self, lane: Lane, flags: MemFlagsData, base: ir::Value) -> ir::Value {
        self.function_builder().ins().atomic_load(machine_type(lane), flags, base)
    }

    fn stack_pointer(&mut self) -> ir::Value {
        self.function_builder().ins().get_stack_pointer(types::I64)
    }

    fn load_access(&mut self, access: Access, flags: MemFlagsData, address: ir::Value) -> ir::Value {
        let kind = Kind::of(access.ty).expect("a scalar access");
        let (cell, result) = (cell_lane(access.bytes), of_in_lane(kind.ir()));
        // A float, or a cell as wide as its integer, loads straight into its view.
        if access.ty != kind.integer() || cell == result {
            return self.raw_load(of_in_lane(scalar_ir(access.ty)), flags, address, 0);
        }
        let extend = if access.signed { Extend::Sign } else { Extend::Zero };
        self.raw_load_extended(cell, extend, result, flags, address, 0)
    }

    fn store_access(&mut self, access: Access, flags: MemFlagsData, address: ir::Value, value: ir::Value) {
        self.raw_store(cell_lane(access.bytes), flags, (value, false), address, 0);
    }

    fn call_words(&mut self, target: CallTarget<ir::Value>, arguments: &[ir::Value]) -> ir::Value {
        let parameters = smallvec::SmallVec::<[Lane; 8]>::from_elem(Lane::I64, arguments.len());
        self.call_lanes(target, (&parameters[..], Lane::I64), arguments)
    }

    fn call_lanes(&mut self, target: CallTarget<ir::Value>, (parameters, result): (&[Lane], Lane), arguments: &[ir::Value]) -> ir::Value {
        let builder = self.function_builder();
        let parameters: Vec<ir::Type> = parameters.iter().map(|lane| machine_type(*lane)).collect();
        let call_conv = leaf_call_conv(builder.func.signature.call_conv);
        let signature = crate::compiled::call_signature::typed_signature(call_conv, &parameters, &[machine_type(result)]);
        let signature = quench_runtime::vm::jit_backend::signature_ref(builder.func, signature);
        self.call_signature(signature, target, arguments)[0]
    }

    fn call_function(&mut self, target: ir::FuncRef, arguments: &[ir::Value]) -> Vec<ir::Value> {
        let builder = self.function_builder();
        let call = builder.ins().call(target, arguments);
        builder.inst_results(call).to_vec()
    }

    /// A slow path's helper symbol where the body names its helpers by symbol (`slow_path_symbol`).
    fn slow_path_symbol(&mut self, address: usize) -> Option<ir::GlobalValue> {
        crate::compiled::slow_path_symbol(self.function_builder().func, address)
    }

    fn symbol_address(&mut self, symbol: ir::GlobalValue) -> ir::Value {
        self.function_builder().ins().symbol_value(types::I64, symbol)
    }

    fn slot_address(&mut self, slot: ir::StackSlot, offset: i32) -> ir::Value {
        self.function_builder().ins().stack_addr(types::I64, slot, offset)
    }

    fn slot_store(&mut self, slot: ir::StackSlot, lane: Lane, value: ir::Value, offset: i32) {
        self.function_builder().ins().stack_store(machine_type(lane), value, slot, offset);
    }

    fn call_signature(&mut self, signature: ir::SigRef, target: CallTarget<ir::Value>, arguments: &[ir::Value]) -> Vec<ir::Value> {
        let builder = self.function_builder();
        let call = builder.ins().call_indirect(signature, target.word(), arguments);
        builder.inst_results(call).to_vec()
    }
});
