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

/// Operators a leaf callee may have to be copied into each call site when
/// the interpreter runs the module: a call costs it a frame and a dispatch,
/// so copying a body pays up to this size. The budget bounds code growth per
/// call site; larger callees keep their call.
const INTERPRETED_LEAF_OPERATORS: usize = 512;

/// The leaf budget for a module lowered for `mode`. Compiled code calls
/// cheaply and allocates registers per function, where a copied body only
/// adds pressure and spills, so compiled lowering keeps every call.
pub(super) fn leaf_budget(mode: crate::ExecutionMode) -> usize {
    match mode {
        crate::ExecutionMode::Interpreted => INTERPRETED_LEAF_OPERATORS,
        crate::ExecutionMode::Compiled => 0,
    }
}

/// Inlining rounds. A function that only called leaves is call-free after a
/// round and may be inlined in the next, so code grows by at most this many
/// nested budgets per call site.
const INLINE_ROUNDS: usize = 2;

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
fn leaves(
    bodies: &[(Vec<ValType>, Vec<Operator<'_>>)],
    signatures: &WasmSignatures,
    budget: usize,
) -> Vec<Option<Leaf>> {
    bodies
        .iter()
        .enumerate()
        .map(|(index, (locals, operators))| {
            let signature = signatures.defined_signature(index as u32)?;
            if operators.len() > budget
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

/// Inline leaf calls throughout a module's bodies, by defined index.
pub(super) fn inline_leaf_calls<'a>(
    mut bodies: Vec<(Vec<ValType>, Vec<Operator<'a>>)>,
    signatures: &WasmSignatures,
    budget: usize,
) -> Option<Vec<(Vec<ValType>, Vec<Operator<'a>>)>> {
    for _ in 0..INLINE_ROUNDS {
        let leaves = leaves(&bodies, signatures, budget);
        if leaves.iter().all(Option::is_none) {
            break;
        }
        // Only bodies that call a leaf are rebuilt; the rest stay in place.
        // A rebuilt caller that is not itself a leaf replaces its old body at
        // once, since no other body inlines it; leaves change only after the
        // round, so every call site of a round inlines the same bodies.
        let mut rebuilt_leaves = Vec::new();
        for index in 0..bodies.len() {
            let (locals, operators) = &bodies[index];
            if !operators
                .iter()
                .any(|operator| leaf_call(operator, index, &leaves, signatures).is_some())
            {
                continue;
            }
            let params = signatures.defined_signature(index as u32)?.params.len();
            let mut locals = locals.clone();
            let operators = inline_body(
                index,
                params,
                &mut locals,
                operators,
                &bodies,
                &leaves,
                signatures,
            )?;
            if leaves[index].is_some() {
                rebuilt_leaves.push((index, (locals, operators)));
            } else {
                bodies[index] = (locals, operators);
            }
        }
        for (index, body) in rebuilt_leaves {
            bodies[index] = body;
        }
    }
    Some(bodies)
}

/// The leaf `operator` calls from `caller`, if it is a call to one.
fn leaf_call<'l>(
    operator: &Operator<'_>,
    caller: usize,
    leaves: &'l [Option<Leaf>],
    signatures: &WasmSignatures,
) -> Option<(usize, &'l Leaf)> {
    let Operator::Call { function_index } = operator else {
        return None;
    };
    let callee = signatures.defined_index(*function_index)? as usize;
    (callee != caller).then_some(())?;
    Some((callee, leaves.get(callee)?.as_ref()?))
}

/// Operators an inlined call to `leaf`, whose body is `body`, expands to:
/// the parameter stores, the resets of locals not assigned first, the
/// block, and the body itself (one operator per callee operator).
fn inlined_length(leaf: &Leaf, body: &[Operator<'_>]) -> usize {
    let resets = leaf
        .assigned_first
        .iter()
        .filter(|assigned| !**assigned)
        .count();
    leaf.params.len() + 2 * resets + 1 + body.len()
}

/// Inline every call to a leaf other than `caller` itself, appending the
/// callees' locals to `locals` after the caller's `params` and locals.
fn inline_body<'a>(
    caller: usize,
    params: usize,
    locals: &mut Vec<ValType>,
    operators: &[Operator<'a>],
    bodies: &[(Vec<ValType>, Vec<Operator<'a>>)],
    leaves: &[Option<Leaf>],
    signatures: &WasmSignatures,
) -> Option<Vec<Operator<'a>>> {
    // The exact length: growing by doubling would hold up to three
    // copies' worth of the body at once.
    let length = operators
        .iter()
        .map(
            |operator| match leaf_call(operator, caller, leaves, signatures) {
                Some((callee, leaf)) => inlined_length(leaf, &bodies[callee].1),
                None => 1,
            },
        )
        .sum();
    let mut inlined = Vec::with_capacity(length);
    for operator in operators {
        let Some((callee, leaf)) = leaf_call(operator, caller, leaves, signatures) else {
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
