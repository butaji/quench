//! Calls to small leaf functions are inlined into the caller's operator
//! stream before lowering. A call becomes: the arguments stored into fresh
//! caller locals, the callee's other locals reset to their defaults, and the
//! callee body as a block of the callee's result type, in which `return`
//! branches to that block. Wasm's structured control keeps every relative
//! depth inside the body valid, because the block stands where the callee's
//! function label stood. Traps, memory and global accesses keep their meaning
//! in the caller's instance; only the activation (and so the call depth that
//! exhausts the stack) disappears.

use wasmparser::{BlockType, Operator, ValType};

use super::{WasmSignatures, WasmType};

/// Operators a leaf callee may have to be copied into each call site. The
/// budget bounds code growth per call site; larger callees keep their call.
const INLINE_LEAF_OPERATORS: usize = 512;

/// A callee that can replace its call sites.
#[derive(Clone)]
pub(super) struct Leaf {
    params: Vec<ValType>,
    result: Option<ValType>,
    /// Declared locals the body assigns before any read; they need no reset
    /// at each inline entry.
    assigned_first: Vec<bool>,
}

fn numeric_value_type(ty: &WasmType) -> Option<ValType> {
    Some(match ty {
        WasmType::I32 => ValType::I32,
        WasmType::I64 => ValType::I64,
        WasmType::F32 => ValType::F32,
        WasmType::F64 => ValType::F64,
        _ => return None,
    })
}

/// Operators a leaf must not contain: calls (which would make it a non-leaf)
/// and exception handling, whose handler ranges stay per function.
fn disqualifies(operator: &Operator<'_>) -> bool {
    matches!(
        operator,
        Operator::Call { .. }
            | Operator::CallIndirect { .. }
            | Operator::CallRef { .. }
            | Operator::ReturnCall { .. }
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

/// The inlinable leaves among the defined functions, by defined index.
pub(super) fn leaves(
    bodies: &[(Vec<ValType>, Vec<Operator<'_>>)],
    signatures: &WasmSignatures,
) -> Vec<Option<Leaf>> {
    bodies
        .iter()
        .enumerate()
        .map(|(index, (locals, operators))| {
            let signature = signatures.defined_signature(index as u32)?;
            if operators.len() > INLINE_LEAF_OPERATORS
                || signature.results.len() > 1
                || operators.iter().any(disqualifies)
                || locals.iter().any(|ty| {
                    !matches!(
                        ty,
                        ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64
                    )
                })
            {
                return None;
            }
            let params = u16::try_from(signature.params.len()).ok()?;
            Some(Leaf {
                assigned_first: super::first_use_assigns(params, locals.len(), operators),
                params: signature
                    .params
                    .iter()
                    .map(numeric_value_type)
                    .collect::<Option<_>>()?,
                result: match signature.results.first() {
                    Some(result) => Some(numeric_value_type(result)?),
                    None => None,
                },
            })
        })
        .collect()
}

fn default_value<'a>(ty: ValType) -> Operator<'a> {
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

/// Inline every call to a leaf other than `caller` itself, appending the
/// callees' locals to `locals` after the caller's `params` and locals.
pub(super) fn inline_leaf_calls<'a>(
    caller: usize,
    params: usize,
    locals: &mut Vec<ValType>,
    operators: &[Operator<'a>],
    bodies: &[(Vec<ValType>, Vec<Operator<'a>>)],
    leaves: &[Option<Leaf>],
    signatures: &WasmSignatures,
) -> Option<Vec<Operator<'a>>> {
    let mut inlined = Vec::with_capacity(operators.len());
    for operator in operators {
        let callee = match operator {
            Operator::Call { function_index } => signatures
                .defined_index(*function_index)
                .map(|index| index as usize)
                .filter(|index| *index != caller),
            _ => None,
        };
        let Some((callee, leaf)) =
            callee.and_then(|callee| Some((callee, leaves.get(callee)?.as_ref()?)))
        else {
            inlined.push(operator.clone());
            continue;
        };
        let (callee_locals, body) = &bodies[callee];
        let base = u32::try_from(params + locals.len()).ok()?;
        let parameters = u32::try_from(leaf.params.len()).ok()?;
        locals.extend(leaf.params.iter().chain(callee_locals).copied());
        for parameter in (0..parameters).rev() {
            inlined.push(Operator::LocalSet {
                local_index: base + parameter,
            });
        }
        for (offset, ty) in callee_locals.iter().enumerate() {
            if leaf.assigned_first[offset] {
                continue;
            }
            inlined.push(default_value(*ty));
            inlined.push(Operator::LocalSet {
                local_index: base + parameters + u32::try_from(offset).ok()?,
            });
        }
        inlined.push(Operator::Block {
            blockty: leaf.result.map_or(BlockType::Empty, BlockType::Type),
        });
        let mut depth = 0_u32;
        for operator in body {
            inlined.push(match operator {
                Operator::LocalGet { local_index } => Operator::LocalGet {
                    local_index: base + local_index,
                },
                Operator::LocalSet { local_index } => Operator::LocalSet {
                    local_index: base + local_index,
                },
                Operator::LocalTee { local_index } => Operator::LocalTee {
                    local_index: base + local_index,
                },
                Operator::Return => Operator::Br {
                    relative_depth: depth,
                },
                Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => {
                    depth += 1;
                    operator.clone()
                }
                Operator::End => {
                    // The body's final `end` closes the inline block.
                    depth = depth.saturating_sub(1);
                    operator.clone()
                }
                operator => operator.clone(),
            });
        }
    }
    Some(inlined)
}
