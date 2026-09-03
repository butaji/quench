//! Build-time-derived region stencil selection.
//!
//! The runtime selector is intentionally boring: canonicalize facts before
//! this boundary, then perform one key lookup and use the ordinary VM on a
//! miss. Instruction selection and CFG reasoning do not belong here.

use crate::stencil_fact::{FactState, RegionId, RegionKey, Stencil};

pub const MAX_RENDERED_REGIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionRecord {
    pub key: RegionKey,
    pub stencil: Stencil,
    pub operations: &'static [crate::ir::Opcode],
    pub entry: u16,
    pub fallthrough: Option<(&'static Stencil, u16)>,
    /// Some regions describe IC data/layout but do not yet contain a complete
    /// executable semantic leaf.  Those rows remain selectable for auditing,
    /// but the renderer must use the canonical fallback for them.
    pub executable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderedRegion {
    pub key: RegionKey,
    pub signature: u64,
    pub address: usize,
}

/// Disposable, fixed-capacity memo table.  Replacement is round-robin and is
/// independent of workload identity, source paths, and hotness thresholds.
#[derive(Clone, Debug)]
pub struct RenderedRegionCache {
    entries: [Option<RenderedRegion>; MAX_RENDERED_REGIONS],
    next: usize,
}

impl Default for RenderedRegionCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderedRegionCache {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_RENDERED_REGIONS],
            next: 0,
        }
    }

    pub fn get(&self, key: RegionKey, signature: u64) -> Option<usize> {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.key == key && entry.signature == signature)
            .map(|entry| entry.address)
    }

    pub fn insert(&mut self, key: RegionKey, signature: u64, address: usize) -> usize {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.key == key && entry.signature == signature)
        {
            entry.address = address;
            return address;
        }
        let index = self.next;
        self.entries[index] = Some(RenderedRegion {
            key,
            signature,
            address,
        });
        self.next = (self.next + 1) % MAX_RENDERED_REGIONS;
        address
    }

    /// Remove one unpublished/invalidated render.  Protection is an explicit
    /// lifecycle edge, so a writable address must not remain visible as a
    /// reusable executable entry when that edge fails.
    pub fn remove(&mut self, key: RegionKey, signature: u64, address: usize) -> bool {
        let Some(index) = self.entries.iter().position(|entry| {
            entry.is_some_and(|entry| {
                entry.key == key && entry.signature == signature && entry.address == address
            })
        }) else {
            return false;
        };
        self.entries[index] = None;
        true
    }

    pub fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }
    pub const fn capacity(&self) -> usize {
        MAX_RENDERED_REGIONS
    }

    pub fn clear(&mut self) {
        self.entries.fill(None);
        self.next = 0;
    }
}

include!(concat!(env!("OUT_DIR"), "/stencil_catalog.rs"));

/// Select an admitted region with one canonical table lookup.
pub fn select_stencil(key: RegionKey) -> Option<&'static Stencil> {
    REGION_TABLE
        .iter()
        .find(|record| record.key == key)
        .map(|record| &record.stencil)
}

pub fn select_region(key: RegionKey) -> Option<&'static RegionRecord> {
    REGION_TABLE.iter().find(|record| record.key == key)
}

/// Execute the selected region through a caller-owned semantic entry point.
/// A miss has exactly one outcome: the complete ordinary interpreter path.
/// Keeping this boundary as a table lookup prevents runtime fact-dependent
/// dispatch chains from growing around individual operations.
pub fn dispatch_region<T, E>(
    key: RegionKey,
    selected: impl FnOnce(&'static RegionRecord) -> Result<T, E>,
    fallback: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    match select_region(key) {
        Some(record) => selected(record),
        None => fallback(),
    }
}

pub fn admitted_region_key(region: RegionId, facts: &[FactState]) -> RegionKey {
    RegionKey::from_facts(region, facts)
}

pub const fn region_table_len() -> usize {
    REGION_TABLE.len()
}

/// Build-generated key for the executable two-region fallthrough example.
pub const fn fallthrough_region_key() -> RegionKey {
    FALLTHROUGH_KEY
}

/// Build-generated key for the named plain-own property leaf.
pub const fn property_region_key() -> RegionKey {
    PROPERTY_KEY
}

/// Build-generated key for the pure tagged-word Move leaf.
pub const fn move_region_key() -> RegionKey {
    MOVE_KEY
}

/// Build-generated entry trampoline shared by every compact baseline
/// instruction. The opcode sequence in the catalog is exhaustive; the
/// trampoline itself only forwards an opaque execution context to Rust.
pub const fn dispatch_region_key() -> RegionKey {
    DISPATCH_KEY
}

/// Build-selected keys for the other proven numeric binary leaves.  These
/// remain catalog lookups; no runtime operation semantics are duplicated here.
pub fn subtract_region_key() -> RegionKey {
    SUBTRACT_KEY
}

pub fn multiply_region_key() -> RegionKey {
    MULTIPLY_KEY
}

pub fn divide_region_key() -> RegionKey {
    DIVIDE_KEY
}

pub fn add_const_region_key() -> RegionKey {
    ADD_CONST_KEY
}

/// Canonical admission table for numeric baseline leaves.  The opcode catalog
/// owns which operations are eligible; callers only ask for the derived key.
pub fn numeric_region_key(opcode: crate::ir::Opcode) -> Option<RegionKey> {
    NUMERIC_REGION_KEYS
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_canonical_and_misses_fall_back() {
        assert!(select_stencil(LOOP_KEY).is_some());
        assert!(select_stencil(RegionKey(0)).is_none());
        assert_eq!(
            LOOP_KEY,
            RegionKey::from_opcodes(
                RegionId(1),
                &[crate::ir::Opcode::Add, crate::ir::Opcode::Return]
            )
        );
    }

    #[test]
    fn numeric_leaf_keys_are_catalog_admissions() {
        for opcode in [
            crate::ir::Opcode::Add,
            crate::ir::Opcode::Sub,
            crate::ir::Opcode::Mul,
            crate::ir::Opcode::Div,
            crate::ir::Opcode::AddConst,
        ] {
            let key = numeric_region_key(opcode).expect("numeric leaf key");
            assert!(select_region(key).is_some());
        }
        assert_eq!(numeric_region_key(crate::ir::Opcode::GetProperty), None);
    }

    #[cfg(not(target_arch = "x86_64"))]
    #[test]
    fn numeric_rows_never_admit_x86_bytes_on_other_isas() {
        for opcode in [
            crate::ir::Opcode::Add,
            crate::ir::Opcode::Sub,
            crate::ir::Opcode::Mul,
            crate::ir::Opcode::Div,
            crate::ir::Opcode::AddConst,
        ] {
            let key = numeric_region_key(opcode).expect("numeric leaf key");
            assert!(!select_region(key).expect("catalog row").executable);
        }
    }

    #[test]
    fn property_row_is_catalog_admitted() {
        let key = RegionKey::from_opcodes(RegionId(2), &[crate::ir::Opcode::GetN]);
        let record = select_region(key).expect("property admission row");
        assert_eq!(record.executable, cfg!(target_arch = "x86_64"));
        assert_eq!(
            record.stencil.bytes.len(),
            if cfg!(target_arch = "x86_64") { 4 } else { 1 }
        );
        assert!(record.stencil.holes.is_empty());
    }

    #[test]
    fn move_row_is_catalog_admitted() {
        let key = RegionKey::from_opcodes(RegionId(8), &[crate::ir::Opcode::Move]);
        let record = select_region(key).expect("move admission row");
        assert_eq!(record.executable, cfg!(target_arch = "x86_64"));
        assert_eq!(
            record.stencil.bytes.len(),
            if cfg!(target_arch = "x86_64") { 4 } else { 1 }
        );
        assert!(record.stencil.holes.is_empty());
    }

    #[test]
    fn dispatch_row_covers_every_compact_opcode() {
        let record = select_region(dispatch_region_key()).expect("dispatch admission row");
        assert_eq!(
            record.operations.len(),
            usize::from(crate::ir::Opcode::COUNT)
        );
        for opcode in 1..=crate::ir::Opcode::COUNT {
            let opcode = crate::ir::Opcode::from_u8(opcode).expect("catalog opcode");
            assert!(record.operations.contains(&opcode));
        }
        assert_eq!(record.executable, cfg!(target_arch = "x86_64"));
        assert_eq!(
            record.stencil.holes.len(),
            if cfg!(target_arch = "x86_64") { 1 } else { 0 }
        );
    }

    #[test]
    fn dispatch_uses_region_sequence_and_falls_back_once() {
        let selected = dispatch_region(
            LOOP_KEY,
            |record| Ok::<_, ()>(record.operations.len()),
            || Ok::<_, ()>(0),
        );
        assert_eq!(selected, Ok(2));
        let ordinary = dispatch_region(RegionKey(0), |_| Ok::<_, ()>(99), || Ok::<_, ()>(7));
        assert_eq!(ordinary, Ok(7));
    }

    #[test]
    fn removing_a_failed_render_does_not_change_bounded_replacement_state() {
        let mut cache = RenderedRegionCache::new();
        let key = RegionKey(17);
        let signature = 23;
        cache.insert(key, signature, 41);
        assert_eq!(cache.len(), 1);
        assert!(cache.remove(key, signature, 41));
        assert_eq!(cache.get(key, signature), None);
        assert_eq!(cache.len(), 0);
        assert!(!cache.remove(key, signature, 41));
    }

    #[test]
    fn reusable_type_pass_reduces_two_distinct_predicates() {
        let checks = [TypeCheck::Number, TypeCheck::Object];
        let first = reduce_type_checks(&checks, |check| {
            if check == TypeCheck::Number {
                PredicateResult::AlwaysTrue
            } else {
                PredicateResult::Unknown
            }
        });
        let second = reduce_type_checks(&checks, |check| {
            if check == TypeCheck::Object {
                PredicateResult::AlwaysFalse
            } else {
                PredicateResult::Unknown
            }
        });
        assert_eq!(first.as_slice(), &[Some(TypeCheck::Object)]);
        assert_eq!(first.always_true(), 1);
        assert_eq!(second.as_slice(), &[Some(TypeCheck::Number)]);
        assert_eq!(second.always_false(), 1);
    }

    #[test]
    fn cfg_rejects_external_entry_into_region_interior() {
        let blocks = [
            RegionBlock {
                id: 10,
                predecessors: &[],
                external_entry: true,
            },
            RegionBlock {
                id: 11,
                predecessors: &[10],
                external_entry: false,
            },
        ];
        assert!(has_single_entry_point(10, &blocks));
        let bad = [
            RegionBlock {
                id: 10,
                predecessors: &[],
                external_entry: true,
            },
            RegionBlock {
                id: 11,
                predecessors: &[10],
                external_entry: true,
            },
        ];
        assert!(!has_single_entry_point(10, &bad));
    }

    #[test]
    fn rendered_region_memo_is_fixed_capacity() {
        let mut cache = RenderedRegionCache::new();
        for index in 0..(MAX_RENDERED_REGIONS + 1) {
            cache.insert(RegionKey(index as u64), 0, index);
        }
        assert_eq!(cache.len(), MAX_RENDERED_REGIONS);
        assert_eq!(cache.get(RegionKey(0), 0), None);
        assert_eq!(
            cache.get(RegionKey(MAX_RENDERED_REGIONS as u64), 0),
            Some(MAX_RENDERED_REGIONS)
        );
    }

    #[test]
    fn promotion_rule_is_fact_only_and_shared() {
        let first = RegionKey(1);
        let second = RegionKey(2);
        assert!(!promotion_admitted(first, first));
        assert!(promotion_admitted(first, second));
        assert_eq!(
            choose_promotion(Some(first), first, false),
            Promotion::Repatch
        );
        assert_eq!(
            choose_promotion(Some(first), second, true),
            Promotion::Repatch
        );
        assert_eq!(
            choose_promotion(Some(first), second, false),
            Promotion::Render
        );
    }
}
