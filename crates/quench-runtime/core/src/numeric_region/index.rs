use super::analysis::{insert_requirement, written_register};
use super::model::{GuardKind, GuardSource, NumericBinary, NumericUnary, RegionOp};
use crate::dynbytecode::Register;
use std::collections::{BTreeMap, BTreeSet};

const MAX_JS_ARRAY_INDEX: f64 = u32::MAX as f64 - 1.0;

#[derive(Clone)]
struct IndexProof {
    roots: BTreeSet<GuardSource>,
    maximum: f64,
}

pub(super) fn derive_index_requirements(
    ops: &[RegionOp],
    requirements: &mut Vec<(GuardSource, GuardKind)>,
) -> Vec<usize> {
    let definitions = definitions(ops);
    let mut proven_sites = Vec::new();
    for (pc, index) in dense_indices(ops) {
        let Some(mut proof) = prove_index(index, &definitions, &mut BTreeSet::new()) else {
            continue;
        };
        if prove_loop_carried_indices(&mut proof, ops, &definitions)
            && strengthen_roots(requirements, proof.roots)
        {
            proven_sites.push(pc);
        }
    }
    proven_sites
}

fn definitions(ops: &[RegionOp]) -> BTreeMap<Register, &RegionOp> {
    ops.iter()
        .filter_map(|op| written_register(op).map(|register| (register, op)))
        .collect()
}

fn dense_indices(ops: &[RegionOp]) -> impl Iterator<Item = (usize, Register)> + '_ {
    ops.iter().filter_map(|op| match op {
        RegionOp::ReadDense { pc, index, .. } | RegionOp::WriteDense { pc, index, .. } => {
            Some((*pc, *index))
        }
        _ => None,
    })
}

fn strengthen_roots(
    requirements: &mut Vec<(GuardSource, GuardKind)>,
    roots: BTreeSet<GuardSource>,
) -> bool {
    let mut strengthened = requirements.iter().cloned().collect::<BTreeMap<_, _>>();
    if !roots
        .into_iter()
        .all(|source| insert_requirement(&mut strengthened, source, GuardKind::ArrayIndex).is_ok())
    {
        return false;
    }
    *requirements = strengthened.into_iter().collect();
    true
}

fn prove_loop_carried_indices(
    proof: &mut IndexProof,
    ops: &[RegionOp],
    definitions: &BTreeMap<Register, &RegionOp>,
) -> bool {
    let mut checked_locals = BTreeSet::new();
    loop {
        let Some(slot) = unchecked_local(&proof.roots, &checked_locals) else {
            return true;
        };
        checked_locals.insert(slot);
        for source in local_writes(ops, slot) {
            let Some(update) = prove_index(source, definitions, &mut BTreeSet::new()) else {
                return false;
            };
            proof.roots.extend(update.roots);
            proof.maximum = proof.maximum.max(update.maximum);
        }
    }
}

fn unchecked_local(roots: &BTreeSet<GuardSource>, checked: &BTreeSet<usize>) -> Option<usize> {
    roots.iter().find_map(|source| match source {
        GuardSource::Local(slot) if !checked.contains(slot) => Some(*slot),
        _ => None,
    })
}

fn local_writes(ops: &[RegionOp], slot: usize) -> impl Iterator<Item = Register> + '_ {
    ops.iter().filter_map(move |op| match op {
        RegionOp::WriteLocal {
            slot: written, src, ..
        } if *written == slot => Some(*src),
        _ => None,
    })
}

fn prove_index(
    register: Register,
    definitions: &BTreeMap<Register, &RegionOp>,
    visiting: &mut BTreeSet<Register>,
) -> Option<IndexProof> {
    if !visiting.insert(register) {
        return None;
    }
    let proof = match definitions.get(&register) {
        Some(RegionOp::ReadLocal { slot, .. }) => index_root(GuardSource::Local(*slot)),
        Some(RegionOp::ReadCaptured { name, .. }) => {
            index_root(GuardSource::Captured(name.clone()))
        }
        Some(RegionOp::Move { src, .. })
        | Some(RegionOp::Unary {
            src,
            kind: NumericUnary::Plus,
            ..
        }) => prove_index(*src, definitions, visiting)?,
        Some(RegionOp::NumberLiteral { bits, .. }) => literal_proof(*bits)?,
        Some(RegionOp::Binary {
            left, right, kind, ..
        }) => binary_proof(*left, *right, *kind, definitions, visiting)?,
        None => index_root(GuardSource::LiveIn(register)),
        _ => return None,
    };
    visiting.remove(&register);
    Some(proof)
}

fn literal_proof(bits: u64) -> Option<IndexProof> {
    let value = f64::from_bits(bits);
    (value.is_finite() && value >= 0.0 && value <= MAX_JS_ARRAY_INDEX && value.trunc() == value)
        .then(|| IndexProof {
            roots: BTreeSet::new(),
            maximum: value,
        })
}

fn binary_proof(
    left: Register,
    right: Register,
    kind: NumericBinary,
    definitions: &BTreeMap<Register, &RegionOp>,
    visiting: &mut BTreeSet<Register>,
) -> Option<IndexProof> {
    let mut left = prove_index(left, definitions, visiting)?;
    let right = prove_index(right, definitions, visiting)?;
    let maximum = match kind {
        NumericBinary::Add => left.maximum + right.maximum,
        NumericBinary::Multiply => left.maximum * right.maximum,
        _ => return None,
    };
    if !maximum.is_finite() {
        return None;
    }
    left.roots.extend(right.roots);
    left.maximum = maximum;
    Some(left)
}

fn index_root(source: GuardSource) -> IndexProof {
    IndexProof {
        roots: BTreeSet::from([source]),
        maximum: MAX_JS_ARRAY_INDEX,
    }
}
