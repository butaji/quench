//! Operator classes of the inlining plan: which operators keep a body from being copied, which
//! count as calls, and the operators and locals a copy of a leaf needs.
use super::*;

/// How `operator` changes the structured nesting it leaves for the operators after it.
pub(super) fn nesting_after(nesting: usize, operator: &Operator<'_>) -> usize {
    match operator {
        Operator::Block { .. }
        | Operator::Loop { .. }
        | Operator::If { .. }
        | Operator::Try { .. }
        | Operator::TryTable { .. } => nesting + 1,
        Operator::End | Operator::Delegate { .. } => nesting.saturating_sub(1),
        _ => nesting,
    }
}

pub(super) fn numeric_value_type(ty: &WasmType) -> Option<ValType> {
    Some(match ty {
        WasmType::I32 => ValType::I32,
        WasmType::I64 => ValType::I64,
        WasmType::F32 => ValType::F32,
        WasmType::F64 => ValType::F64,
        _ => return None,
    })
}

pub(super) fn numeric(ty: &ValType) -> bool {
    matches!(
        ty,
        ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64
    )
}

/// Operators that keep a body from being copied: tail calls, which would
/// replace the caller's activation, and exception handling, whose handler
/// ranges stay per function.
pub(super) fn disqualifies(operator: &Operator<'_>) -> bool {
    matches!(
        operator,
        Operator::ReturnCall { .. }
            | Operator::ReturnCallIndirect { .. }
            | Operator::ReturnCallRef { .. }
            | Operator::Try { .. }
            | Operator::Catch { .. }
            | Operator::CatchAll
            | Operator::Delegate { .. }
            | Operator::Rethrow { .. }
            | Operator::Throw { .. }
            | Operator::ThrowRef
            | Operator::TryTable { .. }
    )
}

pub(super) fn calls(operator: &Operator<'_>) -> bool {
    matches!(
        operator,
        Operator::Call { .. } | Operator::CallIndirect { .. } | Operator::CallRef { .. }
    )
}

pub(super) fn default_value<'a>(ty: ValType) -> Operator<'a> {
    match ty {
        ValType::I32 => Operator::I32Const { value: 0 },
        ValType::I64 => Operator::I64Const { value: 0 },
        ValType::F32 => Operator::F32Const {
            value: wasmparser::Ieee32::from(0.0),
        },
        ValType::F64 => Operator::F64Const {
            value: wasmparser::Ieee64::from(0.0),
        },
        ValType::V128 | ValType::Ref(_) => unreachable!("leaf locals are numeric"),
    }
}

/// Adds the ascending indices of `from` to the ascending `into`.
pub(super) fn union(into: &mut Vec<u32>, from: &[u32]) {
    if from.iter().all(|index| into.binary_search(index).is_ok()) {
        return;
    }
    into.extend_from_slice(from);
    into.sort_unstable();
    into.dedup();
}
