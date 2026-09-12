use super::guard::GuardPlan;
use super::index::derive_index_requirements;
use super::model::*;
use super::rewrite::reduce_block_local;
use crate::dynbytecode::{DynCode, DynOp, Literal, Register, UnaryKind};
use std::collections::{BTreeMap, BTreeSet};
use std::env;

pub const MIN_NUMERIC_DENSE_REGION_OPS: usize = 8;
pub const MIN_NUMERIC_BLOCK_REGION_OPS: usize = 8;
const NUMERIC_REGION_TRACE_ENV: &str = "QUENCH_NUMERIC_REGION_TRACE";
const NUMERIC_REGION_OP_TRACE_ENV: &str = "QUENCH_NUMERIC_REGION_OP_TRACE";
const NUMERIC_REGION_STATS_ENV: &str = "QUENCH_NUMERIC_REGION_STATS";

pub fn trace_enabled() -> bool {
    env::var_os(NUMERIC_REGION_TRACE_ENV).is_some()
}

pub fn op_trace_enabled() -> bool {
    env::var_os(NUMERIC_REGION_OP_TRACE_ENV).is_some()
}

pub fn stats_enabled() -> bool {
    env::var_os(NUMERIC_REGION_STATS_ENV).is_some()
}

pub fn analyze(code: &DynCode) -> RegionAnalysis {
    let backedges = collect_backedges(code);
    let mut analysis = RegionAnalysis::default();
    for (start, latches) in backedges {
        let end = latches.last().copied().unwrap() + 1;
        match quote_loop(code, start, end) {
            Ok(region) => analysis.loops.push(region),
            Err(reason) => analysis.rejected.push(RejectedLoop { start, end, reason }),
        }
    }
    analysis
}

fn collect_backedges(code: &DynCode) -> BTreeMap<usize, Vec<usize>> {
    let mut backedges = BTreeMap::<usize, Vec<usize>>::new();
    for (pc, instruction) in code.ops.iter().enumerate() {
        if let Some(target) = branch_target(&instruction.op)
            && target <= pc
        {
            backedges.entry(target).or_default().push(pc);
        }
    }
    backedges
}

fn quote_loop(code: &DynCode, start: usize, end: usize) -> Result<QuotedLoop, RejectReason> {
    if end.saturating_sub(start) < MIN_NUMERIC_DENSE_REGION_OPS {
        return Err(RejectReason::TooShort);
    }
    reject_external_entries(code, start, end)?;
    reject_nested_loops(code, start, end)?;
    let exit = unique_exit(code, start, end)?;
    let ops = code.ops[start..end]
        .iter()
        .enumerate()
        .map(|(offset, instruction)| quote_op(start + offset, &instruction.op))
        .collect::<Result<Vec<_>, _>>()?;
    if !ops.iter().any(|op| {
        matches!(
            op,
            RegionOp::ReadDense { .. }
                | RegionOp::WriteDense { .. }
                | RegionOp::ReadStatic { .. }
                | RegionOp::WriteStatic { .. }
        )
    }) {
        return Err(RejectReason::NoDenseAccess);
    }
    reject_escaping_temporaries(code, start, end, &ops)?;
    // Loop regions can side-exit through a generic semantic suffix.  Keep
    // their original register state until zero-code frame-state hints can
    // reconstruct renamed values at every exit (Task 330).
    let rewrite_stats = RewriteStats::default();
    let internal_local_sources = derive_internal_local_sources(&ops);
    let internal_number_loads = derive_internal_number_loads(&ops, &internal_local_sources);
    let mut requirements = derive_requirements(&ops, &internal_local_sources)?;
    let proven_index_sites = derive_index_requirements(&ops, &mut requirements);
    let property_requirements = derive_property_requirements(&ops, &internal_local_sources)?;
    let body: Region<NumericDenseState, NumericDenseState> =
        ops.into_iter().fold(identity(), |region, op| {
            region + Region::new(RegionNode::Op(op))
        });
    Ok(QuotedLoop {
        start,
        end,
        exit,
        region: Region::new(RegionNode::Trace {
            header: start,
            exit,
            body: Box::new(body.node),
        }),
        requirements,
        internal_local_sources,
        internal_number_loads,
        proven_index_sites,
        property_requirements,
        rewrite_stats,
    })
}

pub fn quote_adjacent_loop(
    code: &DynCode,
    prefix_start: usize,
    loop_start: usize,
    loop_end: usize,
) -> Result<QuotedLoop, RejectReason> {
    if prefix_start >= loop_start || loop_start >= loop_end {
        return Err(RejectReason::TooShort);
    }
    reject_external_entries(code, prefix_start, loop_end)?;
    reject_nested_loops_except(code, prefix_start, loop_end, loop_start)?;
    let exit = unique_exit(code, prefix_start, loop_end)?;
    let prefix = quote_ops(code, prefix_start, loop_start)?;
    let loop_body = quote_ops(code, loop_start, loop_end)?;
    let mut ops = prefix.clone();
    ops.extend(loop_body.clone());
    reject_escaping_temporaries(code, prefix_start, loop_end, &ops)?;
    let internal_local_sources = derive_internal_local_sources(&ops);
    let internal_number_loads = derive_internal_number_loads(&ops, &internal_local_sources);
    let mut requirements = derive_requirements(&ops, &internal_local_sources)?;
    let proven_index_sites = derive_index_requirements(&ops, &mut requirements);
    let property_requirements = derive_property_requirements(&ops, &internal_local_sources)?;
    let region = sequence_nodes(prefix, loop_start, exit, loop_body);
    Ok(QuotedLoop {
        start: prefix_start,
        end: loop_end,
        exit,
        region,
        requirements,
        internal_local_sources,
        internal_number_loads,
        proven_index_sites,
        property_requirements,
        rewrite_stats: RewriteStats::default(),
    })
}

fn quote_ops(code: &DynCode, start: usize, end: usize) -> Result<Vec<RegionOp>, RejectReason> {
    code.ops
        .get(start..end)
        .ok_or(RejectReason::TooShort)?
        .iter()
        .enumerate()
        .map(|(offset, instruction)| quote_op(start + offset, &instruction.op))
        .collect()
}

fn sequence_nodes(
    prefix: Vec<RegionOp>,
    loop_start: usize,
    exit: usize,
    loop_body: Vec<RegionOp>,
) -> Region<NumericDenseState, NumericDenseState> {
    let prefix = prefix.into_iter().map(RegionNode::Op).collect();
    let loop_body = loop_body.into_iter().map(RegionNode::Op).collect();
    Region::new(RegionNode::Seq(vec![
        RegionNode::Seq(prefix),
        RegionNode::Trace {
            header: loop_start,
            exit,
            body: Box::new(RegionNode::Seq(loop_body)),
        },
    ]))
}

pub fn quote_block(code: &DynCode, start: usize, end: usize) -> Result<QuotedLoop, RejectReason> {
    if end.saturating_sub(start) < MIN_NUMERIC_BLOCK_REGION_OPS {
        return Err(RejectReason::TooShort);
    }
    let ops = code
        .ops
        .get(start..end)
        .ok_or(RejectReason::TooShort)?
        .iter()
        .enumerate()
        .map(|(offset, instruction)| quote_op(start + offset, &instruction.op))
        .collect::<Result<Vec<_>, _>>()?;
    if !ops
        .iter()
        .any(|op| matches!(op, RegionOp::Unary { .. } | RegionOp::Binary { .. }))
    {
        return Err(RejectReason::NoNumericOperation);
    }
    reject_escaping_temporaries(code, start, end, &ops)?;
    let (ops, rewrite_stats) = reduce_block_local(ops);
    let internal_local_sources = derive_internal_local_sources(&ops);
    let internal_number_loads = derive_internal_number_loads(&ops, &internal_local_sources);
    let mut requirements = derive_requirements(&ops, &internal_local_sources)?;
    let proven_index_sites = derive_index_requirements(&ops, &mut requirements);
    let property_requirements = derive_property_requirements(&ops, &internal_local_sources)?;
    let region = ops.into_iter().fold(identity(), |region, op| {
        region + Region::new(RegionNode::Op(op))
    });
    Ok(QuotedLoop {
        start,
        end,
        exit: end,
        region,
        requirements,
        internal_local_sources,
        internal_number_loads,
        proven_index_sites,
        property_requirements,
        rewrite_stats,
    })
}

fn quote_op(pc: usize, op: &DynOp) -> Result<RegionOp, RejectReason> {
    let unsupported = || RejectReason::UnsupportedOpcode {
        pc,
        opcode: op.name(),
    };
    Ok(match op {
        DynOp::LoadLiteral {
            dst,
            value: Literal::Number(value),
        } => RegionOp::NumberLiteral {
            pc,
            dst: *dst,
            bits: value.to_bits(),
        },
        DynOp::LoadLocal { dst, slot } => RegionOp::ReadLocal {
            pc,
            dst: *dst,
            slot: *slot,
        },
        DynOp::LoadName { dst, name } => RegionOp::ReadCaptured {
            pc,
            dst: *dst,
            name: name.clone(),
        },
        DynOp::DeclareLocal { slot, src } | DynOp::StoreLocal { slot, src } => {
            RegionOp::WriteLocal {
                pc,
                slot: *slot,
                src: *src,
            }
        }
        DynOp::Move { dst, src } => RegionOp::Move {
            pc,
            dst: *dst,
            src: *src,
        },
        DynOp::Unary { dst, src, kind } => RegionOp::Unary {
            pc,
            dst: *dst,
            src: *src,
            kind: match kind {
                UnaryKind::Plus => NumericUnary::Plus,
                UnaryKind::Negate => NumericUnary::Negate,
                UnaryKind::BitNot => NumericUnary::BitNot,
                UnaryKind::Not | UnaryKind::Typeof | UnaryKind::Void => return Err(unsupported()),
            },
        },
        DynOp::Binary {
            dst,
            left,
            right,
            kind,
        } => RegionOp::Binary {
            pc,
            dst: *dst,
            left: *left,
            right: *right,
            kind: NumericBinary::from_op(*kind).ok_or_else(unsupported)?,
        },
        DynOp::GetComputed { dst, object, key } => RegionOp::ReadDense {
            pc,
            dst: *dst,
            object: *object,
            index: *key,
        },
        DynOp::SetComputed { object, key, src } => RegionOp::WriteDense {
            pc,
            object: *object,
            index: *key,
            src: *src,
        },
        DynOp::GetStatic { dst, object, key } => RegionOp::ReadStatic {
            pc,
            dst: *dst,
            object: *object,
            key: key.clone(),
        },
        DynOp::SetStatic { object, key, src } => RegionOp::WriteStatic {
            pc,
            object: *object,
            key: key.clone(),
            src: *src,
        },
        DynOp::Jump { target } => RegionOp::Jump {
            pc,
            target: *target,
        },
        DynOp::JumpIfFalse { test, target } => RegionOp::JumpIfFalse {
            pc,
            test: *test,
            target: *target,
        },
        _ => return Err(unsupported()),
    })
}

fn branch_target(op: &DynOp) -> Option<usize> {
    match op {
        DynOp::Jump { target }
        | DynOp::JumpIfFalse { target, .. }
        | DynOp::PushHandler { target } => Some(*target),
        DynOp::ForInNext { done, .. } => Some(*done),
        _ => None,
    }
}

fn reject_external_entries(code: &DynCode, start: usize, end: usize) -> Result<(), RejectReason> {
    for (pc, instruction) in code.ops.iter().enumerate() {
        if (start..end).contains(&pc) {
            continue;
        }
        if let Some(target) = branch_target(&instruction.op)
            && (start + 1..end).contains(&target)
        {
            return Err(RejectReason::ExternalEntry { pc, target });
        }
    }
    Ok(())
}

fn reject_nested_loops(code: &DynCode, start: usize, end: usize) -> Result<(), RejectReason> {
    for (pc, instruction) in code.ops[start..end].iter().enumerate() {
        let pc = start + pc;
        if let Some(target) = branch_target(&instruction.op)
            && target <= pc
            && target != start
        {
            return Err(RejectReason::NestedLoop { pc, target });
        }
    }
    Ok(())
}

fn reject_nested_loops_except(
    code: &DynCode,
    start: usize,
    end: usize,
    allowed_header: usize,
) -> Result<(), RejectReason> {
    for (offset, instruction) in code.ops[start..end].iter().enumerate() {
        let pc = start + offset;
        if let Some(target) = branch_target(&instruction.op)
            && target <= pc
            && target != allowed_header
        {
            return Err(RejectReason::NestedLoop { pc, target });
        }
    }
    Ok(())
}

fn unique_exit(code: &DynCode, start: usize, end: usize) -> Result<usize, RejectReason> {
    let exits = code.ops[start..end]
        .iter()
        .filter_map(|instruction| branch_target(&instruction.op))
        .filter(|target| *target < start || *target >= end)
        .collect::<BTreeSet<_>>();
    match exits.len() {
        0 => Ok(end),
        1 => Ok(*exits.first().unwrap()),
        _ => Err(RejectReason::MultipleExits),
    }
}

fn reject_escaping_temporaries(
    code: &DynCode,
    _start: usize,
    end: usize,
    ops: &[RegionOp],
) -> Result<(), RejectReason> {
    let definitions = ops
        .iter()
        .filter_map(written_register)
        .collect::<BTreeSet<_>>();
    for register in definitions {
        if code.ops[end..]
            .iter()
            .any(|instruction| op_reads_register(&instruction.op, register))
        {
            return Err(RejectReason::EscapingTemporary(register));
        }
    }
    Ok(())
}

pub(super) fn written_register(op: &RegionOp) -> Option<Register> {
    match op {
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

fn derive_requirements(
    ops: &[RegionOp],
    internal_local_sources: &BTreeMap<Register, Register>,
) -> Result<Vec<(GuardSource, GuardKind)>, RejectReason> {
    let definitions = ops
        .iter()
        .filter_map(|op| written_register(op).map(|register| (register, op)))
        .collect::<BTreeMap<_, _>>();
    let mut requirements = BTreeMap::new();
    for op in ops {
        for (register, kind) in required_registers(op) {
            resolve_requirement(
                register,
                kind,
                &definitions,
                internal_local_sources,
                &mut requirements,
                &mut BTreeSet::new(),
            )?;
        }
    }
    Ok(requirements.into_iter().collect())
}

fn derive_internal_number_loads(
    ops: &[RegionOp],
    internal_local_sources: &BTreeMap<Register, Register>,
) -> Vec<usize> {
    let demanded_registers = ops
        .iter()
        .flat_map(required_registers)
        .filter_map(|(register, kind)| {
            matches!(kind, GuardKind::Number | GuardKind::ArrayIndex).then_some(register)
        })
        .collect::<BTreeSet<_>>();
    ops.iter()
        .filter_map(|op| match op {
            RegionOp::ReadLocal { pc, dst, .. }
                if demanded_registers.contains(dst) && internal_local_sources.contains_key(dst) =>
            {
                Some(*pc)
            }
            _ => None,
        })
        .collect()
}

fn derive_internal_local_sources(ops: &[RegionOp]) -> BTreeMap<Register, Register> {
    type LocalDefinitions = BTreeMap<usize, Register>;

    let pc_indices = ops
        .iter()
        .enumerate()
        .map(|(index, op)| (operation_pc(op), index))
        .collect::<BTreeMap<_, _>>();
    let mut incoming = vec![None::<LocalDefinitions>; ops.len()];
    let Some(entry) = incoming.first_mut() else {
        return BTreeMap::new();
    };
    *entry = Some(LocalDefinitions::new());
    let mut worklist = vec![0];
    while let Some(index) = worklist.pop() {
        let Some(mut outgoing) = incoming[index].clone() else {
            continue;
        };
        if let RegionOp::WriteLocal { slot, src, .. } = &ops[index] {
            outgoing.insert(*slot, *src);
        }
        for successor in region_successors(ops, &pc_indices, index) {
            let changed = match &mut incoming[successor] {
                Some(previous) => intersect_local_definitions(previous, &outgoing),
                slot @ None => {
                    *slot = Some(outgoing.clone());
                    true
                }
            };
            if changed {
                worklist.push(successor);
            }
        }
    }

    ops.iter()
        .enumerate()
        .filter_map(|(index, op)| match op {
            RegionOp::ReadLocal { dst, slot, .. } => incoming[index]
                .as_ref()
                .and_then(|definitions| definitions.get(slot))
                .copied()
                .map(|source| (*dst, source)),
            _ => None,
        })
        .collect()
}

fn region_successors(
    ops: &[RegionOp],
    pc_indices: &BTreeMap<usize, usize>,
    index: usize,
) -> Vec<usize> {
    let fallthrough = (index + 1 < ops.len()).then_some(index + 1);
    let branch = match &ops[index] {
        RegionOp::Jump { target, .. } | RegionOp::JumpIfFalse { target, .. } => {
            pc_indices.get(target).copied()
        }
        _ => None,
    };
    match &ops[index] {
        RegionOp::Jump { .. } => branch.into_iter().collect(),
        RegionOp::JumpIfFalse { .. } => branch.into_iter().chain(fallthrough).collect(),
        _ => fallthrough.into_iter().collect(),
    }
}

fn intersect_local_definitions(
    current: &mut BTreeMap<usize, Register>,
    incoming: &BTreeMap<usize, Register>,
) -> bool {
    let previous_len = current.len();
    current.retain(|slot, source| incoming.get(slot) == Some(source));
    current.len() != previous_len
}

fn operation_pc(op: &RegionOp) -> usize {
    match op {
        RegionOp::Elided { pc }
        | RegionOp::NumberLiteral { pc, .. }
        | RegionOp::ReadLocal { pc, .. }
        | RegionOp::WriteLocal { pc, .. }
        | RegionOp::ReadCaptured { pc, .. }
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

fn derive_property_requirements(
    ops: &[RegionOp],
    internal_local_sources: &BTreeMap<Register, Register>,
) -> Result<Vec<PropertyRequirement>, RejectReason> {
    let result_kinds = derive_property_result_kinds(ops, internal_local_sources)?;
    let written_locals = ops
        .iter()
        .filter_map(|op| match op {
            RegionOp::WriteLocal { slot, .. } => Some(*slot),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut definitions = BTreeMap::<Register, Option<GuardSource>>::new();
    let mut requirements = Vec::new();
    for op in ops {
        let property = match op {
            RegionOp::ReadStatic {
                pc, object, key, ..
            } => Some((*pc, *object, key, StaticPropertyAccess::Read)),
            RegionOp::WriteStatic {
                pc, object, key, ..
            } => Some((*pc, *object, key, StaticPropertyAccess::Write)),
            _ => None,
        };
        if let Some((pc, object, key, access)) = property {
            let source = match definitions.get(&object) {
                Some(Some(source)) => source.clone(),
                Some(None) | None => {
                    return Err(RejectReason::TypeConflict(GuardSource::LiveIn(object)));
                }
            };
            if matches!(source, GuardSource::Local(slot) if written_locals.contains(&slot)) {
                return Err(RejectReason::TypeConflict(source));
            }
            requirements.push(PropertyRequirement {
                pc,
                source,
                key: key.clone(),
                access,
                value_kind: result_kinds
                    .get(&pc)
                    .copied()
                    .unwrap_or(PropertyValueKind::TriviallyCopyable),
            });
        }
        match op {
            RegionOp::ReadLocal { dst, slot, .. } => {
                definitions.insert(*dst, Some(GuardSource::Local(*slot)));
            }
            RegionOp::ReadCaptured { dst, name, .. } => {
                definitions.insert(*dst, Some(GuardSource::Captured(name.clone())));
            }
            RegionOp::Move { dst, src, .. } => {
                definitions.insert(*dst, definitions.get(src).cloned().flatten());
            }
            _ => {
                if let Some(destination) = written_register(op) {
                    definitions.insert(destination, None);
                }
            }
        }
    }
    Ok(requirements)
}

fn derive_property_result_kinds(
    ops: &[RegionOp],
    internal_local_sources: &BTreeMap<Register, Register>,
) -> Result<BTreeMap<usize, PropertyValueKind>, RejectReason> {
    let mut definitions = BTreeMap::<Register, usize>::new();
    let mut result_kinds = BTreeMap::new();
    for op in ops {
        for (register, kind) in required_registers(op) {
            let Some(property_pc) = definitions.get(&register).copied() else {
                continue;
            };
            let value_kind = match kind {
                GuardKind::Number | GuardKind::ArrayIndex => PropertyValueKind::Number,
                GuardKind::DenseArray => PropertyValueKind::DenseArray,
            };
            let merged = result_kinds
                .get(&property_pc)
                .copied()
                .unwrap_or(PropertyValueKind::TriviallyCopyable)
                .merge(value_kind)
                .ok_or_else(|| RejectReason::TypeConflict(GuardSource::LiveIn(register)))?;
            result_kinds.insert(property_pc, merged);
        }
        match op {
            RegionOp::ReadStatic { pc, dst, .. } => {
                definitions.insert(*dst, *pc);
            }
            RegionOp::Move { dst, src, .. } => {
                if let Some(property_pc) = definitions.get(src).copied() {
                    definitions.insert(*dst, property_pc);
                } else {
                    definitions.remove(dst);
                }
            }
            RegionOp::ReadLocal { dst, .. } => {
                if let Some(property_pc) = internal_local_sources
                    .get(dst)
                    .and_then(|source| definitions.get(source))
                    .copied()
                {
                    definitions.insert(*dst, property_pc);
                } else {
                    definitions.remove(dst);
                }
            }
            _ => {
                if let Some(destination) = written_register(op) {
                    definitions.remove(&destination);
                }
            }
        }
    }
    Ok(result_kinds)
}

fn required_registers(op: &RegionOp) -> Vec<(Register, GuardKind)> {
    match op {
        RegionOp::Unary { src, .. } => vec![(*src, GuardKind::Number)],
        RegionOp::Binary { left, right, .. } => {
            vec![(*left, GuardKind::Number), (*right, GuardKind::Number)]
        }
        RegionOp::ReadDense { object, index, .. } => vec![
            (*object, GuardKind::DenseArray),
            (*index, GuardKind::Number),
        ],
        RegionOp::WriteDense {
            object, index, src, ..
        } => vec![
            (*object, GuardKind::DenseArray),
            (*index, GuardKind::Number),
            (*src, GuardKind::Number),
        ],
        RegionOp::WriteStatic { src, .. } => vec![(*src, GuardKind::Number)],
        _ => Vec::new(),
    }
}

fn resolve_requirement<'a>(
    register: Register,
    kind: GuardKind,
    definitions: &BTreeMap<Register, &'a RegionOp>,
    internal_local_sources: &BTreeMap<Register, Register>,
    requirements: &mut BTreeMap<GuardSource, GuardKind>,
    visiting: &mut BTreeSet<(Register, GuardKind)>,
) -> Result<(), RejectReason> {
    if !visiting.insert((register, kind)) {
        return Ok(());
    }
    let source = match definitions.get(&register) {
        Some(RegionOp::ReadLocal { slot, .. }) => {
            if let Some(source) = internal_local_sources.get(&register) {
                return resolve_requirement(
                    *source,
                    kind,
                    definitions,
                    internal_local_sources,
                    requirements,
                    visiting,
                );
            }
            Some(GuardSource::Local(*slot))
        }
        Some(RegionOp::ReadCaptured { name, .. }) => Some(GuardSource::Captured(name.clone())),
        Some(RegionOp::Move { src, .. }) => {
            return resolve_requirement(
                *src,
                kind,
                definitions,
                internal_local_sources,
                requirements,
                visiting,
            );
        }
        Some(RegionOp::NumberLiteral { .. }) if kind == GuardKind::Number => None,
        Some(RegionOp::Unary { .. } | RegionOp::ReadDense { .. }) if kind == GuardKind::Number => {
            None
        }
        Some(RegionOp::ReadStatic { .. })
            if matches!(kind, GuardKind::Number | GuardKind::DenseArray) =>
        {
            None
        }
        Some(RegionOp::Binary { kind: binary, .. })
            if kind == GuardKind::Number && !binary.result_is_boolean() =>
        {
            None
        }
        Some(_) => return Err(RejectReason::TypeConflict(GuardSource::LiveIn(register))),
        None => Some(GuardSource::LiveIn(register)),
    };
    if let Some(source) = source {
        insert_requirement(requirements, source, kind)?;
    }
    Ok(())
}

pub(super) fn insert_requirement(
    requirements: &mut BTreeMap<GuardSource, GuardKind>,
    source: GuardSource,
    kind: GuardKind,
) -> Result<(), RejectReason> {
    if let Some(previous) = requirements.get(&source).copied() {
        let merged = match (previous, kind) {
            (GuardKind::Number, GuardKind::ArrayIndex)
            | (GuardKind::ArrayIndex, GuardKind::Number)
            | (GuardKind::ArrayIndex, GuardKind::ArrayIndex) => GuardKind::ArrayIndex,
            (left, right) if left == right => left,
            _ => return Err(RejectReason::TypeConflict(source)),
        };
        requirements.insert(source, merged);
    } else {
        requirements.insert(source, kind);
    }
    Ok(())
}

fn op_reads_register(op: &DynOp, register: Register) -> bool {
    match op {
        DynOp::LoadLiteral { .. }
        | DynOp::LoadName { .. }
        | DynOp::LoadLocal { .. }
        | DynOp::LoadThis { .. }
        | DynOp::NewArray { .. }
        | DynOp::NewObject { .. }
        | DynOp::MakeClosure { .. }
        | DynOp::RegExp { .. }
        | DynOp::Jump { .. }
        | DynOp::PushHandler { .. }
        | DynOp::PopHandler
        | DynOp::Catch { .. }
        | DynOp::Rethrow => false,
        DynOp::NewArrayFromRegisters { elements, .. } => {
            elements.iter().flatten().any(|source| *source == register)
        }
        DynOp::NewObjectFromRegisters { values, .. } => values.contains(&register),
        DynOp::DeclareName { src, .. }
        | DynOp::DeclareLocal { src, .. }
        | DynOp::StoreName { src, .. }
        | DynOp::StoreLocal { src, .. }
        | DynOp::Unary { src, .. }
        | DynOp::Throw { src } => *src == register,
        DynOp::Move { src, .. } => *src == register,
        DynOp::Binary { left, right, .. }
        | DynOp::InstanceOf { left, right, .. }
        | DynOp::In { left, right, .. } => *left == register || *right == register,
        DynOp::GetStatic { object, .. } | DynOp::DeleteStatic { object, .. } => *object == register,
        DynOp::GetComputed { object, key, .. } | DynOp::DeleteComputed { object, key, .. } => {
            *object == register || *key == register
        }
        DynOp::SetStatic { object, src, .. } => *object == register || *src == register,
        DynOp::SetComputed {
            object, key, src, ..
        } => *object == register || *key == register || *src == register,
        DynOp::Call {
            callee,
            receiver,
            args,
            ..
        } => *callee == register || *receiver == register || args.contains(&register),
        DynOp::Construct { callee, args, .. } => *callee == register || args.contains(&register),
        DynOp::JumpIfFalse { test, .. } => *test == register,
        DynOp::ForInInit { object, .. } => *object == register,
        DynOp::ForInNext { iterator, .. } => *iterator == register,
        DynOp::Return { src } => *src == Some(register),
    }
}

pub fn trace(code: &DynCode) {
    if !trace_enabled() {
        return;
    }
    let analysis = analyze(code);
    for region in &analysis.loops {
        let guard = GuardPlan::from_loop(region);
        eprintln!(
            "NUMERIC_REGION:eligible start={} end={} exit={} ops={} rewrites={:?} guards={:?} properties={:?}",
            guard.start,
            region.end,
            guard.exit,
            region.op_count(),
            region.rewrite_stats(),
            guard.requirements(),
            region.property_requirements(),
        );
        if op_trace_enabled() {
            eprintln!("NUMERIC_REGION:operations {:?}", region.operations());
        }
    }
    for rejected in &analysis.rejected {
        eprintln!(
            "NUMERIC_REGION:rejected start={} end={} reason={:?}",
            rejected.start, rejected.end, rejected.reason
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY_PC: usize = 0;
    const STORE_PC: usize = 1;
    const LOAD_PC: usize = 2;
    const EXIT_PC: usize = 3;
    const LOCAL_SLOT: usize = 4;
    const SOURCE_REGISTER: Register = 5;
    const DESTINATION_REGISTER: Register = 6;

    #[test]
    fn unique_reaching_local_definition_reuses_the_producer_context() {
        let ops = vec![
            RegionOp::NumberLiteral {
                pc: ENTRY_PC,
                dst: SOURCE_REGISTER,
                bits: 1.0_f64.to_bits(),
            },
            RegionOp::WriteLocal {
                pc: STORE_PC,
                slot: LOCAL_SLOT,
                src: SOURCE_REGISTER,
            },
            RegionOp::ReadLocal {
                pc: LOAD_PC,
                dst: DESTINATION_REGISTER,
                slot: LOCAL_SLOT,
            },
            RegionOp::Jump {
                pc: EXIT_PC,
                target: ENTRY_PC,
            },
        ];
        assert_eq!(
            derive_internal_local_sources(&ops).get(&DESTINATION_REGISTER),
            Some(&SOURCE_REGISTER)
        );
    }

    #[test]
    fn branch_that_skips_a_store_keeps_the_local_external() {
        const TEST_REGISTER: Register = 7;
        let ops = vec![
            RegionOp::JumpIfFalse {
                pc: ENTRY_PC,
                test: TEST_REGISTER,
                target: LOAD_PC,
            },
            RegionOp::WriteLocal {
                pc: STORE_PC,
                slot: LOCAL_SLOT,
                src: SOURCE_REGISTER,
            },
            RegionOp::ReadLocal {
                pc: LOAD_PC,
                dst: DESTINATION_REGISTER,
                slot: LOCAL_SLOT,
            },
        ];
        assert!(!derive_internal_local_sources(&ops).contains_key(&DESTINATION_REGISTER));
    }
}
