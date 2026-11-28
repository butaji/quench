//! The staged single-opcode aliases whose operations are crate-private (the Wasm families): each is
//! true for exactly one opcode, whose record decodes to the operation its doc names. The guard
//! `crate_alias_selectors_name_one_row` (`tests/staged_guards/coverage.rs`) runs `crate_alias_failures`,
//! and the debt scan counts these aliases' opcodes as derived only because it does;
//! `alias_selectors_name_one_row` pins the public ones.

use super::wasm_access::WasmAccessOperation;
use super::wasm_float::WasmFloatOperation;
use super::wasm_simd::VectorOperation;
use super::wasm_state::WasmStateOperation;
use super::{Constant, DecodeContext, Operation};
use crate::bytecode::{Op, WideInstruction};

type Row = Operation<u64, ()>;
type Is = fn(&Row) -> bool;

/// The names of the pinned aliases, in order, and every way one fails its pin.
#[doc(hidden)]
pub fn crate_alias_failures() -> (Vec<&'static str>, Vec<String>) {
    let aliases: [(&str, fn(Op) -> bool, Is); 11] = [
        ("wasm_branch_table", super::wasm_branch_table, |o| matches!(o, Operation::BranchTable(_))),
        ("wasm_simd", super::wasm_simd, |o| matches!(o, Operation::WasmVector(VectorOperation::Operator { .. }))),
        ("wasm_simd_shuffle", super::wasm_simd_shuffle, |o| matches!(o, Operation::WasmVector(VectorOperation::Shuffle { .. }))),
        ("wasm_memory_address", super::wasm_memory_address, |o| matches!(o, Operation::WasmAccess(WasmAccessOperation::EffectiveAddress { .. }))),
        ("wasm_memory_load", super::wasm_memory_load, |o| matches!(o, Operation::WasmAccess(WasmAccessOperation::GenericLoad { .. }))),
        ("wasm_memory_store", super::wasm_memory_store, |o| matches!(o, Operation::WasmAccess(WasmAccessOperation::GenericStore { .. }))),
        ("wasm_f64_unary", super::wasm_f64_unary, |o| matches!(o, Operation::WasmFloat(WasmFloatOperation::Unary { .. }))),
        ("wasm_instance_binding", super::wasm_instance_binding, |o| matches!(o, Operation::WasmState(WasmStateOperation::InstanceBinding { .. }))),
        ("wasm_fill_registers", super::wasm_fill_registers, |o| matches!(o, Operation::WasmState(WasmStateOperation::Fill { .. }))),
        ("wasm_array_new_defaulted", super::wasm_array_new_defaulted, |_| true),
        ("wasm_array_new_fixed", super::wasm_array_new_fixed, |_| true),
    ];
    let constants = [Constant::untyped(crate::Value::UNDEFINED); 4];
    // A generic access's offset is a raw 64-bit constant: a double's bits, which no tag claims.
    let offsets = [Constant::untyped(crate::Value::number(0.5)); 4];
    let contexts = [DecodeContext::EMPTY, DecodeContext { constants: &constants, constant_at: None, method_sites: &[] },
        DecodeContext { constants: &offsets, constant_at: None, method_sites: &[] }];
    let decodes = |op: Op, is: Is| {
        (0..48u32).any(|imm| [(1u16, 2u16, 3u16), (0, 1, 0), (1, 0, 0)].iter().any(|&(a, b, c)| {
            let record = WideInstruction::new(op, a, b, c, imm);
            contexts.iter().any(|&context| Row::decode(record, context).is_some_and(|o| is(&o)))
                || Row::decode_lane(record).is_some_and(|o| is(&o))
        }))
    };
    let mut failures = Vec::new();
    for &(name, alias, is) in &aliases {
        let ops: Vec<Op> = (0..Op::COUNT).map(|index| Op::from_index(index).unwrap()).filter(|&op| alias(op)).collect();
        match ops.as_slice() {
            // The GC constructions are their step's (`GcStep::of`), which the aliases read: one opcode each.
            [_] if name.starts_with("wasm_array_new") => {}
            [op] if decodes(*op, is) => {}
            [op] => failures.push(format!("{name}: {} does not decode to the documented operation", Op::NAMES[*op as usize])),
            _ => failures.push(format!("{name}: true for {} opcodes, not one", ops.len())),
        }
    }
    (aliases.iter().map(|(name, ..)| *name).collect(), failures)
}
