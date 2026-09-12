use super::model::{NumericBinary, NumericUnary, RegionOp, RewriteStats};
use crate::dynbytecode::Register;
use std::collections::{BTreeMap, BTreeSet};

/// Bounds compile-time metadata for an unusually large quoted block.  Reaching
/// the bound merely stops learning new facts; it never changes semantics.
const MAX_BLOCK_LOCAL_FACTS: usize = 256;
const NEXT_PC_DISTANCE: usize = 1;
const CONDITION_PATTERN_WIDTH: usize = 4;
const LOCAL_UPDATE_PATTERN_WIDTH: usize = 4;
const AARCH64_BURNED_REGISTER_INDEX_BITS: u32 = 12;
const MAX_AARCH64_BURNED_REGISTER_INDEX: Register =
    ((1_u32 << AARCH64_BURNED_REGISTER_INDEX_BITS) - 1) as Register;
const ENABLE_LOCAL_LOAD_FORWARDING: bool = true;
const ENABLE_CAPTURED_LOAD_REUSE: bool = false;
const ENABLE_LITERAL_REUSE: bool = false;
const ENABLE_COPY_PROPAGATION: bool = false;
const ENABLE_PURE_EXPRESSION_REUSE: bool = false;
const ENABLE_HEAP_LOAD_REUSE: bool = false;
const ENABLE_LOCAL_STORE_ELIMINATION: bool = false;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PureValueKey {
    NumberLiteral(u64),
    Unary(NumericUnary, Register),
    Binary(NumericBinary, Register, Register),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum HeapLocationKey {
    Dense(Register, Register),
    Static(Register, String),
}

#[derive(Default)]
struct Facts {
    aliases: BTreeMap<Register, Register>,
    locals: BTreeMap<usize, Register>,
    captured: BTreeMap<String, Register>,
    pure_values: BTreeMap<PureValueKey, Register>,
    heap_values: BTreeMap<HeapLocationKey, Register>,
    pending_local_stores: BTreeMap<usize, usize>,
}

impl Facts {
    fn clear(&mut self) {
        self.aliases.clear();
        self.locals.clear();
        self.captured.clear();
        self.pure_values.clear();
        self.heap_values.clear();
        self.pending_local_stores.clear();
    }

    fn representative(&self, register: Register) -> Register {
        self.aliases.get(&register).copied().unwrap_or(register)
    }

    fn bind_alias(&mut self, destination: Register, representative: Register) {
        if destination != representative && self.aliases.len() < MAX_BLOCK_LOCAL_FACTS {
            self.aliases.insert(destination, representative);
        }
    }

    fn kill_register(&mut self, register: Register) {
        self.aliases.remove(&register);
        self.aliases
            .retain(|_, representative| *representative != register);
        self.locals
            .retain(|_, representative| *representative != register);
        self.captured
            .retain(|_, representative| *representative != register);
        self.pure_values.retain(|key, representative| {
            *representative != register && !pure_key_uses_register(key, register)
        });
        self.heap_values.retain(|key, representative| {
            *representative != register && !heap_key_uses_register(key, register)
        });
    }

    fn remember_local(&mut self, slot: usize, register: Register) {
        if self.locals.len() < MAX_BLOCK_LOCAL_FACTS || self.locals.contains_key(&slot) {
            self.locals.insert(slot, register);
        }
    }

    fn remember_captured(&mut self, name: String, register: Register) {
        if self.captured.len() < MAX_BLOCK_LOCAL_FACTS || self.captured.contains_key(&name) {
            self.captured.insert(name, register);
        }
    }

    fn remember_pure(&mut self, key: PureValueKey, register: Register) {
        if self.pure_values.len() < MAX_BLOCK_LOCAL_FACTS || self.pure_values.contains_key(&key) {
            self.pure_values.insert(key, register);
        }
    }

    fn remember_heap(&mut self, key: HeapLocationKey, register: Register) {
        if self.heap_values.len() < MAX_BLOCK_LOCAL_FACTS || self.heap_values.contains_key(&key) {
            self.heap_values.insert(key, register);
        }
    }
}

fn pure_key_uses_register(key: &PureValueKey, register: Register) -> bool {
    match key {
        PureValueKey::NumberLiteral(_) => false,
        PureValueKey::Unary(_, source) => *source == register,
        PureValueKey::Binary(_, left, right) => *left == register || *right == register,
    }
}

fn heap_key_uses_register(key: &HeapLocationKey, register: Register) -> bool {
    match key {
        HeapLocationKey::Dense(object, index) => *object == register || *index == register,
        HeapLocationKey::Static(object, _) => *object == register,
    }
}

/// Reduce one quoted control-flow region as a sequence of independent basic
/// blocks.  The result remains the same RegionOp language: this is a pure
/// quote-to-quote macro expansion, not a second optimizer IR.
pub(super) fn reduce_block_local(ops: Vec<RegionOp>) -> (Vec<RegionOp>, RewriteStats) {
    let block_ids = derive_block_ids(&ops);
    let cross_block_registers = cross_block_registers(&ops, &block_ids);
    let protected_operations = protected_superinstruction_operations(&ops);
    let mut protected_registers = protected_source_registers(&ops, &protected_operations);
    // Dense element stencils can leave through the general semantic suffix on
    // an index/bounds miss.  Every original operand used by that suffix must
    // therefore be materialized at the side exit.  This is the local form of
    // a deoptimization frame-state constraint.
    protected_registers.extend(fallback_live_registers(&ops, &block_ids));
    // Until every handler operand is a CopyPatch hole, handlers without a
    // burned operand still read their original InlineSite fields.  Definitions
    // feeding those fields cannot be renamed by this reducer.
    protected_registers.extend(site_bound_source_registers(&ops));
    let mut facts = Facts::default();
    let mut output = Vec::with_capacity(ops.len());
    let mut stats = RewriteStats::default();
    let mut previous_block = None;

    for (index, operation) in ops.into_iter().enumerate() {
        let block = block_ids[index];
        if previous_block.is_some_and(|previous| previous != block) {
            facts.clear();
        }
        previous_block = Some(block);

        let protected = protected_operations[index];
        let rewritten = if protected {
            operation
        } else {
            rewrite_sources(operation, &facts)
        };
        let can_elide = destination(&rewritten).is_some_and(|destination| {
            !protected
                && !cross_block_registers.contains(&destination)
                && !protected_registers.contains(&destination)
        });

        let emitted = reduce_operation(
            rewritten,
            can_elide,
            protected,
            &mut facts,
            &mut output,
            &mut stats,
        );
        let terminates_block = matches!(
            emitted,
            RegionOp::Jump { .. } | RegionOp::JumpIfFalse { .. }
        );
        output.push(emitted);
        if terminates_block {
            facts.clear();
        }
    }

    (output, stats)
}

fn reduce_operation(
    operation: RegionOp,
    can_elide: bool,
    protected: bool,
    facts: &mut Facts,
    output: &mut [RegionOp],
    stats: &mut RewriteStats,
) -> RegionOp {
    let pc = operation_pc(&operation);
    match operation {
        RegionOp::Elided { .. } => operation,
        RegionOp::NumberLiteral { dst, bits, .. } => {
            let key = PureValueKey::NumberLiteral(bits);
            let existing = facts.pure_values.get(&key).copied();
            facts.kill_register(dst);
            if can_elide
                && ENABLE_LITERAL_REUSE
                && let Some(representative) = existing
            {
                facts.bind_alias(dst, representative);
                stats.literals_reused += 1;
                RegionOp::Elided { pc }
            } else {
                facts.remember_pure(key, dst);
                RegionOp::NumberLiteral { pc, dst, bits }
            }
        }
        RegionOp::ReadLocal { dst, slot, .. } => {
            facts.pending_local_stores.remove(&slot);
            let existing = facts.locals.get(&slot).copied();
            facts.kill_register(dst);
            if can_elide
                && ENABLE_LOCAL_LOAD_FORWARDING
                && let Some(representative) = existing
            {
                facts.bind_alias(dst, representative);
                stats.local_loads_forwarded += 1;
                RegionOp::Elided { pc }
            } else {
                facts.remember_local(slot, dst);
                RegionOp::ReadLocal { pc, dst, slot }
            }
        }
        RegionOp::ReadCaptured { dst, name, .. } => {
            // A name lookup can observe a current-frame binding, so it is an
            // effect barrier for dead local stores even though repeated reads
            // of the resolved numeric slot are stable within this region.
            facts.pending_local_stores.clear();
            let existing = facts.captured.get(&name).copied();
            facts.kill_register(dst);
            if can_elide
                && ENABLE_CAPTURED_LOAD_REUSE
                && let Some(representative) = existing
            {
                facts.bind_alias(dst, representative);
                stats.captured_loads_reused += 1;
                RegionOp::Elided { pc }
            } else {
                facts.remember_captured(name.clone(), dst);
                RegionOp::ReadCaptured { pc, dst, name }
            }
        }
        RegionOp::Move { dst, src, .. } => {
            let representative = facts.representative(src);
            facts.kill_register(dst);
            if can_elide && ENABLE_COPY_PROPAGATION {
                facts.bind_alias(dst, representative);
                stats.copies_propagated += 1;
                RegionOp::Elided { pc }
            } else {
                RegionOp::Move {
                    pc,
                    dst,
                    src: representative,
                }
            }
        }
        RegionOp::Unary { dst, src, kind, .. } => {
            let key = PureValueKey::Unary(kind, src);
            let existing = facts.pure_values.get(&key).copied();
            facts.kill_register(dst);
            if can_elide
                && ENABLE_PURE_EXPRESSION_REUSE
                && let Some(representative) = existing
            {
                facts.bind_alias(dst, representative);
                stats.pure_expressions_reused += 1;
                RegionOp::Elided { pc }
            } else {
                facts.remember_pure(key, dst);
                RegionOp::Unary { pc, dst, src, kind }
            }
        }
        RegionOp::Binary {
            dst,
            left,
            right,
            kind,
            ..
        } => {
            let key = PureValueKey::Binary(kind, left, right);
            let existing = facts.pure_values.get(&key).copied();
            facts.kill_register(dst);
            if can_elide
                && ENABLE_PURE_EXPRESSION_REUSE
                && let Some(representative) = existing
            {
                facts.bind_alias(dst, representative);
                stats.pure_expressions_reused += 1;
                RegionOp::Elided { pc }
            } else {
                facts.remember_pure(key, dst);
                RegionOp::Binary {
                    pc,
                    dst,
                    left,
                    right,
                    kind,
                }
            }
        }
        RegionOp::ReadDense {
            dst, object, index, ..
        } => {
            let key = HeapLocationKey::Dense(object, index);
            let existing = facts.heap_values.get(&key).copied();
            facts.kill_register(dst);
            if can_elide
                && ENABLE_HEAP_LOAD_REUSE
                && let Some(representative) = existing
            {
                facts.bind_alias(dst, representative);
                stats.heap_loads_reused += 1;
                RegionOp::Elided { pc }
            } else {
                facts.remember_heap(key, dst);
                RegionOp::ReadDense {
                    pc,
                    dst,
                    object,
                    index,
                }
            }
        }
        RegionOp::ReadStatic {
            dst, object, key, ..
        } => {
            let location = HeapLocationKey::Static(object, key.clone());
            let existing = facts.heap_values.get(&location).copied();
            facts.kill_register(dst);
            if can_elide
                && ENABLE_HEAP_LOAD_REUSE
                && let Some(representative) = existing
            {
                facts.bind_alias(dst, representative);
                stats.heap_loads_reused += 1;
                RegionOp::Elided { pc }
            } else {
                facts.remember_heap(location, dst);
                RegionOp::ReadStatic {
                    pc,
                    dst,
                    object,
                    key,
                }
            }
        }
        RegionOp::WriteLocal { slot, src, .. } => {
            if ENABLE_LOCAL_STORE_ELIMINATION
                && !protected
                && let Some(previous) = facts.pending_local_stores.remove(&slot)
                && let RegionOp::WriteLocal {
                    pc: previous_pc, ..
                } = output[previous]
            {
                output[previous] = RegionOp::Elided { pc: previous_pc };
                stats.local_stores_eliminated += 1;
            }
            facts.captured.clear();
            facts.remember_local(slot, src);
            if protected {
                facts.pending_local_stores.remove(&slot);
            } else if facts.pending_local_stores.len() < MAX_BLOCK_LOCAL_FACTS {
                facts.pending_local_stores.insert(slot, output.len());
            }
            RegionOp::WriteLocal { pc, slot, src }
        }
        RegionOp::WriteDense {
            object, index, src, ..
        } => {
            // Unknown aliasing between arrays makes a write kill every known
            // heap value.  We deliberately do not forward the written value
            // until the array-bounds effect is represented explicitly.
            facts.heap_values.clear();
            RegionOp::WriteDense {
                pc,
                object,
                index,
                src,
            }
        }
        RegionOp::WriteStatic {
            object, key, src, ..
        } => {
            facts.heap_values.clear();
            RegionOp::WriteStatic {
                pc,
                object,
                key,
                src,
            }
        }
        RegionOp::Jump { target, .. } => RegionOp::Jump { pc, target },
        RegionOp::JumpIfFalse { test, target, .. } => RegionOp::JumpIfFalse { pc, test, target },
    }
}

fn rewrite_sources(operation: RegionOp, facts: &Facts) -> RegionOp {
    let representative = |register| facts.representative(register);
    match operation {
        RegionOp::WriteLocal { pc, slot, src } => RegionOp::WriteLocal {
            pc,
            slot,
            src: representative(src),
        },
        RegionOp::Move { pc, dst, src } => RegionOp::Move {
            pc,
            dst,
            src: representative(src),
        },
        RegionOp::Unary { pc, dst, src, kind } => RegionOp::Unary {
            pc,
            dst,
            src: representative(src),
            kind,
        },
        RegionOp::Binary {
            pc,
            dst,
            left,
            right,
            kind,
        } => RegionOp::Binary {
            pc,
            dst,
            left: representative(left),
            right: representative(right),
            kind,
        },
        RegionOp::ReadDense {
            pc,
            dst,
            object,
            index,
        } => RegionOp::ReadDense {
            pc,
            dst,
            object: representative(object),
            index: representative(index),
        },
        RegionOp::WriteDense {
            pc,
            object,
            index,
            src,
        } => RegionOp::WriteDense {
            pc,
            object: representative(object),
            index: representative(index),
            src: representative(src),
        },
        RegionOp::ReadStatic {
            pc,
            dst,
            object,
            key,
        } => RegionOp::ReadStatic {
            pc,
            dst,
            object: representative(object),
            key,
        },
        RegionOp::WriteStatic {
            pc,
            object,
            key,
            src,
        } => RegionOp::WriteStatic {
            pc,
            object: representative(object),
            key,
            src: representative(src),
        },
        RegionOp::JumpIfFalse { pc, test, target } => RegionOp::JumpIfFalse {
            pc,
            test: representative(test),
            target,
        },
        operation => operation,
    }
}

fn derive_block_ids(ops: &[RegionOp]) -> Vec<usize> {
    let targets = ops
        .iter()
        .filter_map(|operation| match operation {
            RegionOp::Jump { target, .. } | RegionOp::JumpIfFalse { target, .. } => Some(*target),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut block = 0;
    let mut previous_terminated = false;
    ops.iter()
        .enumerate()
        .map(|(index, operation)| {
            if index != 0 && (previous_terminated || targets.contains(&operation_pc(operation))) {
                block += 1;
            }
            previous_terminated = matches!(
                operation,
                RegionOp::Jump { .. } | RegionOp::JumpIfFalse { .. }
            );
            block
        })
        .collect()
}

fn cross_block_registers(ops: &[RegionOp], block_ids: &[usize]) -> BTreeSet<Register> {
    let mut definitions = BTreeMap::<Register, BTreeSet<usize>>::new();
    let mut uses = BTreeMap::<Register, BTreeSet<usize>>::new();
    for (operation, block) in ops.iter().zip(block_ids.iter().copied()) {
        if let Some(destination) = destination(operation) {
            definitions.entry(destination).or_default().insert(block);
        }
        for source in source_registers(operation) {
            uses.entry(source).or_default().insert(block);
        }
    }
    definitions
        .into_iter()
        .filter_map(|(register, definition_blocks)| {
            let crosses = definition_blocks.len() != 1
                || uses.get(&register).is_some_and(|use_blocks| {
                    use_blocks
                        .iter()
                        .any(|block| !definition_blocks.contains(block))
                });
            crosses.then_some(register)
        })
        .collect()
}

fn protected_superinstruction_operations(ops: &[RegionOp]) -> Vec<bool> {
    let mut protected = vec![false; ops.len()];
    for start in 0..ops.len() {
        if condition_pattern(ops, start) {
            protected[start..start + CONDITION_PATTERN_WIDTH].fill(true);
        }
        if local_update_pattern(ops, start) {
            protected[start..start + LOCAL_UPDATE_PATTERN_WIDTH].fill(true);
        }
    }
    protected
}

fn condition_pattern(ops: &[RegionOp], start: usize) -> bool {
    let Some(window) = ops.get(start..start + CONDITION_PATTERN_WIDTH) else {
        return false;
    };
    let [
        RegionOp::ReadLocal {
            pc, dst: left_dst, ..
        },
        right_operation,
        RegionOp::Binary {
            dst: compare_dst,
            left,
            right,
            kind,
            ..
        },
        RegionOp::JumpIfFalse { test, .. },
    ] = window
    else {
        return false;
    };
    let Some(right_dst) = (match right_operation {
        RegionOp::ReadLocal { dst, .. }
        | RegionOp::ReadCaptured { dst, .. }
        | RegionOp::NumberLiteral { dst, .. } => Some(*dst),
        _ => None,
    }) else {
        return false;
    };
    consecutive_pcs(window, *pc)
        && kind.result_is_boolean()
        && left == left_dst
        && *right == right_dst
        && test == compare_dst
}

fn local_update_pattern(ops: &[RegionOp], start: usize) -> bool {
    let Some(window) = ops.get(start..start + LOCAL_UPDATE_PATTERN_WIDTH) else {
        return false;
    };
    let [
        RegionOp::ReadLocal {
            pc, dst: load_dst, ..
        },
        RegionOp::NumberLiteral {
            dst: literal_dst, ..
        },
        RegionOp::Binary {
            dst: binary_dst,
            left,
            right,
            kind,
            ..
        },
        RegionOp::WriteLocal { src, .. },
    ] = window
    else {
        return false;
    };
    consecutive_pcs(window, *pc)
        && matches!(kind, NumericBinary::Add | NumericBinary::Subtract)
        && left == load_dst
        && right == literal_dst
        && src == binary_dst
}

fn consecutive_pcs(ops: &[RegionOp], start_pc: usize) -> bool {
    ops.iter().enumerate().all(|(offset, operation)| {
        start_pc
            .checked_add(offset * NEXT_PC_DISTANCE)
            .is_some_and(|pc| operation_pc(operation) == pc)
    })
}

fn protected_source_registers(ops: &[RegionOp], protected: &[bool]) -> BTreeSet<Register> {
    ops.iter()
        .zip(protected)
        .filter(|(_, protected)| **protected)
        .flat_map(|(operation, _)| source_registers(operation))
        .collect()
}

fn fallback_live_registers(ops: &[RegionOp], block_ids: &[usize]) -> BTreeSet<Register> {
    let mut live = BTreeSet::new();
    for (exit_index, operation) in ops.iter().enumerate() {
        if !matches!(
            operation,
            RegionOp::ReadDense { .. } | RegionOp::WriteDense { .. }
        ) {
            continue;
        }
        let exit_block = block_ids[exit_index];
        for (suffix_operation, _) in ops[exit_index..]
            .iter()
            .zip(block_ids[exit_index..].iter().copied())
            .take_while(|(_, block)| *block == exit_block)
        {
            live.extend(source_registers(suffix_operation));
        }
    }
    live
}

fn site_bound_source_registers(ops: &[RegionOp]) -> BTreeSet<Register> {
    ops.iter()
        .filter(|operation| !operation_burns_all_sources(operation))
        .flat_map(source_registers)
        .collect()
}

fn operation_burns_all_sources(operation: &RegionOp) -> bool {
    let registers_fit = |registers: &[Register]| {
        registers
            .iter()
            .all(|register| *register <= MAX_AARCH64_BURNED_REGISTER_INDEX)
    };
    match operation {
        RegionOp::Unary { dst, src, .. } => registers_fit(&[*dst, *src]),
        RegionOp::Binary {
            dst,
            left,
            right,
            kind,
            ..
        } => {
            matches!(
                kind,
                NumericBinary::Add
                    | NumericBinary::Subtract
                    | NumericBinary::Multiply
                    | NumericBinary::Divide
                    | NumericBinary::Equal
                    | NumericBinary::NotEqual
                    | NumericBinary::StrictEqual
                    | NumericBinary::StrictNotEqual
                    | NumericBinary::Less
                    | NumericBinary::LessEqual
                    | NumericBinary::Greater
                    | NumericBinary::GreaterEqual
            ) && registers_fit(&[*dst, *left, *right])
        }
        // These operations have no source register to rename.
        RegionOp::Elided { .. }
        | RegionOp::NumberLiteral { .. }
        | RegionOp::ReadLocal { .. }
        | RegionOp::ReadCaptured { .. }
        | RegionOp::Jump { .. } => true,
        // Store/move slot bounds and every heap/branch operand still come from
        // InlineSite in at least one catalog variant.
        RegionOp::WriteLocal { .. }
        | RegionOp::Move { .. }
        | RegionOp::ReadDense { .. }
        | RegionOp::WriteDense { .. }
        | RegionOp::ReadStatic { .. }
        | RegionOp::WriteStatic { .. }
        | RegionOp::JumpIfFalse { .. } => false,
    }
}

fn source_registers(operation: &RegionOp) -> Vec<Register> {
    match operation {
        RegionOp::WriteLocal { src, .. } | RegionOp::Unary { src, .. } => vec![*src],
        RegionOp::Move { src, .. } => vec![*src],
        RegionOp::Binary { left, right, .. } => vec![*left, *right],
        RegionOp::ReadDense { object, index, .. } => vec![*object, *index],
        RegionOp::WriteDense {
            object, index, src, ..
        } => vec![*object, *index, *src],
        RegionOp::ReadStatic { object, .. } => vec![*object],
        RegionOp::WriteStatic { object, src, .. } => vec![*object, *src],
        RegionOp::JumpIfFalse { test, .. } => vec![*test],
        RegionOp::Elided { .. }
        | RegionOp::NumberLiteral { .. }
        | RegionOp::ReadLocal { .. }
        | RegionOp::ReadCaptured { .. }
        | RegionOp::Jump { .. } => Vec::new(),
    }
}

fn destination(operation: &RegionOp) -> Option<Register> {
    match operation {
        RegionOp::NumberLiteral { dst, .. }
        | RegionOp::ReadLocal { dst, .. }
        | RegionOp::ReadCaptured { dst, .. }
        | RegionOp::Move { dst, .. }
        | RegionOp::Unary { dst, .. }
        | RegionOp::Binary { dst, .. }
        | RegionOp::ReadDense { dst, .. }
        | RegionOp::ReadStatic { dst, .. } => Some(*dst),
        _ => None,
    }
}

fn operation_pc(operation: &RegionOp) -> usize {
    match operation {
        RegionOp::Elided { pc }
        | RegionOp::NumberLiteral { pc, .. }
        | RegionOp::ReadLocal { pc, .. }
        | RegionOp::ReadCaptured { pc, .. }
        | RegionOp::WriteLocal { pc, .. }
        | RegionOp::Move { pc, .. }
        | RegionOp::Unary { pc, .. }
        | RegionOp::Binary { pc, .. }
        | RegionOp::ReadDense { pc, .. }
        | RegionOp::WriteDense { pc, .. }
        | RegionOp::ReadStatic { pc, .. }
        | RegionOp::WriteStatic { pc, .. }
        | RegionOp::Jump { pc, .. }
        | RegionOp::JumpIfFalse { pc, .. } => *pc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST_PC: usize = 0;
    const FIRST_REGISTER: Register = 0;
    const SECOND_REGISTER: Register = 1;
    const RESULT_REGISTER: Register = 2;
    const FIRST_LOCAL: usize = 0;

    #[test]
    fn forwards_repeated_local_load_and_rewrites_users() {
        let (ops, stats) = reduce_block_local(vec![
            RegionOp::ReadLocal {
                pc: FIRST_PC,
                dst: FIRST_REGISTER,
                slot: FIRST_LOCAL,
            },
            RegionOp::ReadLocal {
                pc: FIRST_PC + 1,
                dst: SECOND_REGISTER,
                slot: FIRST_LOCAL,
            },
            RegionOp::Binary {
                pc: FIRST_PC + 2,
                dst: RESULT_REGISTER,
                left: FIRST_REGISTER,
                right: SECOND_REGISTER,
                kind: NumericBinary::Multiply,
            },
        ]);
        assert_eq!(stats.local_loads_forwarded, 1);
        assert!(matches!(ops[1], RegionOp::Elided { pc: 1 }));
        assert!(matches!(
            ops[2],
            RegionOp::Binary {
                left: FIRST_REGISTER,
                right: FIRST_REGISTER,
                ..
            }
        ));
    }

    #[test]
    fn forwards_store_to_load_for_a_burned_numeric_user() {
        let (ops, stats) = reduce_block_local(vec![
            RegionOp::NumberLiteral {
                pc: FIRST_PC,
                dst: FIRST_REGISTER,
                bits: 1.0f64.to_bits(),
            },
            RegionOp::WriteLocal {
                pc: FIRST_PC + 1,
                slot: FIRST_LOCAL,
                src: FIRST_REGISTER,
            },
            RegionOp::ReadLocal {
                pc: FIRST_PC + 2,
                dst: SECOND_REGISTER,
                slot: FIRST_LOCAL,
            },
            RegionOp::Binary {
                pc: FIRST_PC + 3,
                dst: RESULT_REGISTER,
                left: SECOND_REGISTER,
                right: SECOND_REGISTER,
                kind: NumericBinary::Multiply,
            },
        ]);
        assert_eq!(stats.local_loads_forwarded, 1);
        assert_eq!(stats.local_stores_eliminated, 0);
        assert!(matches!(ops[2], RegionOp::Elided { .. }));
        assert!(matches!(
            ops[3],
            RegionOp::Binary {
                left: FIRST_REGISTER,
                right: FIRST_REGISTER,
                ..
            }
        ));
    }

    #[test]
    fn heap_write_invalidates_load_cse() {
        let (ops, stats) = reduce_block_local(vec![
            RegionOp::ReadDense {
                pc: FIRST_PC,
                dst: RESULT_REGISTER,
                object: FIRST_REGISTER,
                index: SECOND_REGISTER,
            },
            RegionOp::WriteDense {
                pc: FIRST_PC + 1,
                object: FIRST_REGISTER,
                index: SECOND_REGISTER,
                src: RESULT_REGISTER,
            },
            RegionOp::ReadDense {
                pc: FIRST_PC + 2,
                dst: RESULT_REGISTER + 1,
                object: FIRST_REGISTER,
                index: SECOND_REGISTER,
            },
        ]);
        assert_eq!(stats.heap_loads_reused, 0);
        assert!(matches!(ops[2], RegionOp::ReadDense { .. }));
    }

    #[test]
    fn branch_boundaries_prevent_cross_block_forwarding() {
        let (ops, stats) = reduce_block_local(vec![
            RegionOp::ReadLocal {
                pc: FIRST_PC,
                dst: FIRST_REGISTER,
                slot: FIRST_LOCAL,
            },
            RegionOp::JumpIfFalse {
                pc: FIRST_PC + 1,
                test: FIRST_REGISTER,
                target: FIRST_PC + 3,
            },
            RegionOp::ReadLocal {
                pc: FIRST_PC + 2,
                dst: SECOND_REGISTER,
                slot: FIRST_LOCAL,
            },
            RegionOp::ReadLocal {
                pc: FIRST_PC + 3,
                dst: RESULT_REGISTER,
                slot: FIRST_LOCAL,
            },
        ]);
        assert_eq!(stats.local_loads_forwarded, 0);
        assert!(
            ops.iter()
                .all(|operation| !matches!(operation, RegionOp::Elided { .. }))
        );
    }

    #[test]
    fn preserves_existing_local_update_superinstruction_shape() {
        let operations = vec![
            RegionOp::ReadLocal {
                pc: FIRST_PC,
                dst: FIRST_REGISTER,
                slot: FIRST_LOCAL,
            },
            RegionOp::NumberLiteral {
                pc: FIRST_PC + 1,
                dst: SECOND_REGISTER,
                bits: 1.0f64.to_bits(),
            },
            RegionOp::Binary {
                pc: FIRST_PC + 2,
                dst: RESULT_REGISTER,
                left: FIRST_REGISTER,
                right: SECOND_REGISTER,
                kind: NumericBinary::Add,
            },
            RegionOp::WriteLocal {
                pc: FIRST_PC + 3,
                slot: FIRST_LOCAL,
                src: RESULT_REGISTER,
            },
        ];
        let (rewritten, stats) = reduce_block_local(operations.clone());
        assert_eq!(rewritten, operations);
        assert_eq!(stats.eliminated_operations(), 0);
    }
}
