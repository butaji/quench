/// Canonical admission table for numeric baseline leaves.  The opcode catalog
/// owns which operations are eligible; callers only ask for the derived key.
pub fn numeric_region_key(opcode: crate::ir::Opcode) -> Option<RegionKey> {
    NUMERIC_REGION_KEYS
        .iter()
        .find_map(|(candidate, key)| (*candidate == opcode).then_some(*key))
}

pub(crate) fn continuation_region_key(opcode: crate::ir::Opcode) -> Option<RegionKey> {
    CONTINUATION_REGION_KEYS
        .iter()
        .find_map(|(candidate, key)| (*candidate == opcode).then_some(*key))
}

/// A region block's incoming edges are represented by IDs.  `external_entry`
/// is precomputed by the build-time CFG pass and is not re-derived at runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionBlock<'a> {
    pub id: u32,
    pub predecessors: &'a [u32],
    pub external_entry: bool,
}

/// Prove that the region has exactly one externally reachable entry point.
pub fn has_single_entry_point(entry: u32, blocks: &[RegionBlock<'_>]) -> bool {
    let mut found = false;
    for block in blocks {
        if block.external_entry {
            if block.id != entry || found {
                return false;
            }
            found = true;
        }
    }
    found
}

/// Type predicates operate on the canonical boxing fact dimension; this is an
/// alias, not a selector-owned copy of the type lattice.
pub use crate::stencil_fact::BaseType as TypeCheck;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PredicateResult {
    AlwaysTrue,
    AlwaysFalse,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReducedChecks {
    checks: [Option<TypeCheck>; 8],
    len: usize,
    always_true: u8,
    always_false: u8,
}

impl ReducedChecks {
    pub fn as_slice(&self) -> &[Option<TypeCheck>] {
        &self.checks[..self.len]
    }

    pub const fn always_true(&self) -> u8 {
        self.always_true
    }

    pub const fn always_false(&self) -> u8 {
        self.always_false
    }
}

/// Named reusable form of the type-based reduction algorithm.  Every region
/// invokes this same pass with its predicate; no region gets bespoke logic.
pub fn reduce_type_checks(
    checks: &[TypeCheck],
    predicate: impl Fn(TypeCheck) -> PredicateResult,
) -> ReducedChecks {
    let mut reduced = ReducedChecks {
        checks: [None; 8],
        len: 0,
        always_true: 0,
        always_false: 0,
    };
    for check in checks.iter().copied() {
        match predicate(check) {
            PredicateResult::AlwaysTrue => {
                reduced.always_true = reduced.always_true.saturating_add(1)
            }
            PredicateResult::AlwaysFalse => {
                reduced.always_false = reduced.always_false.saturating_add(1)
            }
            PredicateResult::Unknown if reduced.len < reduced.checks.len() => {
                reduced.checks[reduced.len] = Some(check);
                reduced.len += 1;
            }
            PredicateResult::Unknown => {}
        }
    }
    reduced
}

/// The fixed, fact-driven admission predicate used by the memoized renderer.
pub fn promotion_admitted(previous: RegionKey, next: RegionKey) -> bool {
    previous != next
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Promotion {
    Repatch,
    Render,
    Ordinary,
}

/// Fixed admission rule: preserve the zero-copy/data-only path whenever the
/// installed hole set can express the new facts; otherwise render only when
/// the canonical key actually differs.
pub fn choose_promotion(
    previous: Option<RegionKey>,
    next: RegionKey,
    holes_cover_fact: bool,
) -> Promotion {
    match previous {
        None => Promotion::Render,
        Some(key) if !promotion_admitted(key, next) => Promotion::Repatch,
        Some(_) if holes_cover_fact => Promotion::Repatch,
        Some(_) => Promotion::Render,
    }
}
