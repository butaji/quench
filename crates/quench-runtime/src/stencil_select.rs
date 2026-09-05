//! Build-time-derived region stencil selection.
//!
//! The runtime selector is intentionally boring: canonicalize facts before
//! this boundary, then perform one key lookup and use the ordinary VM on a
//! miss. Instruction selection and CFG reasoning do not belong here.

use crate::facts::OperationEffect;
use crate::stencil_fact::{FactState, RegionId, RegionKey, Stencil};

pub const MAX_RENDERED_REGIONS: usize = 16;

/// Physical calling convention declared by a stencil row.  Selection uses
/// the same generated declaration for opcode shape and ABI, so a scalar leaf
/// can never be accidentally invoked with the region-context pointer ABI.
/// Physical boundary properties shared by selection, validation, and the
/// invocation wrappers.  These are consequences of the declared ABI, not a
/// second semantic implementation of the operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AbiContract {
    /// Number of pointer arguments carrying the erased context. Scalar leaves
    /// use typed FP/word arguments and therefore have no context pointer; the
    /// pointed-to `repr(C)` records have their own independently verified
    /// field layout.
    pub context_arg_words: u8,
    /// Whether VM register state is preserved by the physical entry itself.
    /// Raw kernels write only their explicit context/live-out fields.
    pub preserves_vm_registers: bool,
    /// Whether an interior operation may call a Rust helper that can allocate,
    /// throw, call JS, or re-enter. Raw kernels must keep this false.
    pub may_call_helper: bool,
    /// Whether a native backedge contains a runtime interruption checkpoint.
    /// The bounded array loop polls its context flag; larger/unknown spans use
    /// the ordinary interruptible loop.
    pub interruptible_backedge: bool,
    /// Hardware registers the template may clobber. A preserving leaf must
    /// declare an empty mask; bridge/raw entries name their bounded scratch
    /// set so exit materialization can be audited separately.
    pub hardware_clobber_mask: u16,
    /// Integer register destinations written by raw machine templates. This
    /// is separate from the SIMD mask above because AArch64 contexts use x0-x6
    /// for address arithmetic while numeric values live in d0-d2.
    pub hardware_gpr_clobber_mask: u16,
    /// Canonical live-out slots published by the physical entry. Bit zero is
    /// the ordinary result slot; wider masks are reserved for region exits.
    pub live_out_mask: u16,
    /// Whether the caller must expose VM roots before a helper-capable entry.
    pub root_materialization_required: bool,
}

macro_rules! region_abi_catalog {
    ($( $name:ident => {
        context: $region_context:expr,
        priority: $priority:expr,
        context_words: $context_words:expr,
        preserves_vm_registers: $preserves_vm_registers:expr,
        may_call_helper: $may_call_helper:expr,
        interruptible_backedge: $interruptible_backedge:expr,
        hardware_clobber_mask: $hardware_clobber_mask:expr,
        hardware_gpr_clobber_mask: $hardware_gpr_clobber_mask:expr,
        live_out_mask: $live_out_mask:expr,
        root_materialization_required: $root_materialization_required:expr
    }),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum RegionAbi { $( $name ),+ }

        impl RegionAbi {
            pub const fn accepts_region_context(self) -> bool {
                match self { $(Self::$name => $region_context),+ }
            }

            pub const fn priority(self) -> u8 {
                match self { $(Self::$name => $priority),+ }
            }

            pub const fn contract(self) -> AbiContract {
                match self {
                    $(Self::$name => AbiContract {
                        context_arg_words: $context_words,
                        preserves_vm_registers: $preserves_vm_registers,
                        may_call_helper: $may_call_helper,
                        interruptible_backedge: $interruptible_backedge,
                        hardware_clobber_mask: $hardware_clobber_mask,
                        hardware_gpr_clobber_mask: $hardware_gpr_clobber_mask,
                        live_out_mask: $live_out_mask,
                        root_materialization_required: $root_materialization_required,
                    }),+
                }
            }
        }
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionRecord {
    /// Stable build-time declaration name used only for diagnostics and
    /// storage attribution; semantic selection remains keyed by `RegionKey`.
    pub name: &'static str,
    pub key: RegionKey,
    pub stencil: Stencil,
    pub operations: &'static [crate::ir::Opcode],
    pub entry: u16,
    /// All legal external entry offsets, generated from the declaration.
    /// Runtime admission may enter only at one of these boundaries.
    pub external_entries: &'static [u16],
    pub fallthrough: Option<(&'static Stencil, u16)>,
    pub abi: RegionAbi,
    /// Canonical semantic-boundary fact for the selected template. Runtime
    /// validation still checks physical instructions fail-closed; a branch is
    /// not treated as a helper merely because its encoding resembles one.
    pub template_calls_helper: bool,
    /// Some regions describe IC data/layout but do not yet contain a complete
    /// executable semantic leaf.  Those rows remain selectable for auditing,
    /// but the renderer must use the canonical fallback for them.
    pub executable: bool,
}

/// The mechanical semantic contract of a generated region row.  The row's
/// operation sequence remains the single source of truth: effects, control
/// shape, and result/live-state requirements are queried from the canonical
/// opcode declarations instead of being copied into a stencil-specific table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionContract {
    pub abi: RegionAbi,
    pub operations: &'static [crate::ir::Opcode],
    pub entry: u16,
    pub external_entries: &'static [u16],
    pub executable: bool,
    pub template_calls_helper: bool,
}

impl RegionContract {
    pub const fn legal_external_entry(self, offset: u16) -> bool {
        if offset != self.entry {
            return false;
        }
        let mut index = 0;
        while index < self.external_entries.len() {
            if self.external_entries[index] == offset {
                return true;
            }
            index += 1;
        }
        false
    }

    pub fn has_effect(self, effect: OperationEffect) -> bool {
        self.operations
            .iter()
            .copied()
            .any(|opcode| opcode.has_effect(effect))
    }

    pub const fn has_control_effect(self) -> bool {
        let mut index = 0;
        while index < self.operations.len() {
            if self.operations[index].has_effect(OperationEffect::Control) {
                return true;
            }
            index += 1;
        }
        false
    }

    pub fn requires_semantic_boundary(self) -> bool {
        self.has_effect(OperationEffect::Allocate)
            || self.has_effect(OperationEffect::MayThrow)
            || self.has_effect(OperationEffect::Observable)
    }

    pub const fn abi_contract(self) -> AbiContract {
        self.abi.contract()
    }

    pub const fn has_single_entry(self) -> bool {
        self.external_entries.len() == 1 && self.external_entries[0] == self.entry
    }

    pub const fn abi_is_well_formed(self) -> bool {
        let abi = self.abi_contract();
        let context_arity_ok = if self.abi.accepts_region_context() {
            abi.context_arg_words == 1
        } else {
            abi.context_arg_words == 0
        };
        context_arity_ok
            && (!abi.preserves_vm_registers
                || (abi.hardware_clobber_mask == 0 && abi.hardware_gpr_clobber_mask == 0))
            && (abi.preserves_vm_registers
                || abi.hardware_clobber_mask != 0
                || abi.hardware_gpr_clobber_mask != 0)
            && (!self.operations.is_empty() || abi.live_out_mask == 0)
            && (!abi.may_call_helper || abi.root_materialization_required)
            && (!abi.interruptible_backedge || self.has_control_effect())
    }
}

impl RegionRecord {
    /// Build the contract from this generated row.  No caller may construct a
    /// second effect/ABI universe: operation effects and control facts are
    /// always read from `ir::Opcode::spec` through this view.
    pub const fn contract(&self) -> RegionContract {
        RegionContract {
            abi: self.abi,
            operations: self.operations,
            entry: self.entry,
            external_entries: self.external_entries,
            executable: self.executable,
            template_calls_helper: self.template_calls_helper,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderedRegion {
    pub key: RegionKey,
    pub signature: u64,
    pub address: usize,
    /// Identity of the executable slab that owns `address`.  A raw address
    /// is not sufficient: an OS may reuse a mapping after an arena is
    /// dropped, so cache entries must never become callable in a new owner.
    pub owner: u64,
}

/// One verified physical selection.  Canonical semantic facts stay in the
/// catalog record; the view only couples those facts to the exact bytes that
/// will be rendered, preventing generated/legacy metadata from being mixed.
#[derive(Clone, Copy, Debug)]
pub struct PhysicalStencilView {
    pub key: RegionKey,
    pub record: &'static RegionRecord,
    pub stencil: &'static Stencil,
    pub generated: bool,
    /// Identity and physical boundary facts travel with the selected bytes.
    /// Callers must not pair an artifact with independently looked-up record
    /// metadata after this point.
    pub abi: RegionAbi,
    pub entry: u16,
    pub external_entries: &'static [u16],
    pub fallthrough: Option<(&'static Stencil, u16)>,
    pub executable: bool,
    pub template_calls_helper: bool,
    pub target: Option<&'static str>,
    pub fingerprint: Option<&'static str>,
}

impl PhysicalStencilView {
    /// Rebuild the semantic contract with the selected physical boundary.
    /// Operation effects remain borrowed from the canonical record; physical
    /// ABI/layout facts come only from this view.
    pub const fn contract(&self) -> RegionContract {
        RegionContract {
            abi: self.abi,
            operations: self.record.operations,
            entry: self.entry,
            external_entries: self.external_entries,
            executable: self.executable,
            template_calls_helper: self.template_calls_helper,
        }
    }
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

    pub fn get_owned(&self, key: RegionKey, signature: u64, owner: u64) -> Option<usize> {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.key == key && entry.signature == signature && entry.owner == owner)
            .map(|entry| entry.address)
    }

    pub fn insert(&mut self, key: RegionKey, signature: u64, address: usize) -> usize {
        self.insert_owned(key, signature, address, 0)
    }

    pub fn insert_owned(
        &mut self,
        key: RegionKey,
        signature: u64,
        address: usize,
        owner: u64,
    ) -> usize {
        if let Some(entry) =
            self.entries.iter_mut().flatten().find(|entry| {
                entry.key == key && entry.signature == signature && entry.owner == owner
            })
        {
            entry.address = address;
            return address;
        }
        let index = self.next;
        self.entries[index] = Some(RenderedRegion {
            key,
            signature,
            address,
            owner,
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

    /// Remove every render owned by a retired executable slab.  Cache entries
    /// are derived lookup data; dropping them at the same ownership boundary
    /// keeps stale generations from consuming the bounded table or being
    /// mistaken for a rebuild hit.
    pub(crate) fn remove_owner(&mut self, owner: u64) -> usize {
        let mut removed = 0;
        for entry in &mut self.entries {
            if entry.is_some_and(|rendered| rendered.owner == owner) {
                *entry = None;
                removed += 1;
            }
        }
        removed
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
include!(concat!(env!("OUT_DIR"), "/stencil_artifacts.rs"));

/// Select an admitted region with one canonical table lookup.
pub fn select_stencil(key: RegionKey) -> Option<&'static Stencil> {
    select_physical(key).map(|view| view.stencil)
}

/// Select bytes only after checking the typed entry ABI.  Callers that retain
/// an ABI-specific entry must use this gate instead of pairing a raw stencil
/// lookup with an independently chosen function signature.
pub fn select_stencil_for_abi(
    key: RegionKey,
    abi: RegionAbi,
) -> Option<&'static Stencil> {
    select_physical(key)
        .filter(|view| view.executable && view.abi == abi)
        .map(|view| view.stencil)
}

pub fn select_physical(key: RegionKey) -> Option<PhysicalStencilView> {
    let record = CANONICAL_REGION_TABLE.iter().find(|record| record.key == key)?;
    let Some(artifact) = BUILD_STENCIL_ARTIFACTS
        .iter()
        .find(|artifact| artifact.key == key && artifact.name == record.name)
    else {
        return Some(legacy_physical_view(key, record));
    };
    // A matching identity reserves the generated representation.  If any
    // ABI, target, layout, or effect contract differs, fail closed instead of
    // silently substituting legacy bytes with generated metadata.
    generated_physical_view(key, record, artifact)
}

fn legacy_physical_view(key: RegionKey, record: &'static RegionRecord) -> PhysicalStencilView {
    PhysicalStencilView {
        key,
        record,
        stencil: &record.stencil,
        generated: false,
        abi: record.abi,
        entry: record.entry,
        external_entries: record.external_entries,
        fallthrough: record.fallthrough,
        executable: record.executable,
        template_calls_helper: record.template_calls_helper,
        target: option_env!("QUENCH_BUILD_TARGET"),
        fingerprint: None,
    }
}

fn generated_physical_view(
    key: RegionKey,
    record: &'static RegionRecord,
    artifact: &'static BuildStencilArtifact,
) -> Option<PhysicalStencilView> {
    let fallthrough = artifact
        .fallthrough
        .as_ref()
        .map(|stencil| (stencil, artifact.fallthrough_entry));
    let metadata_matches = artifact.name == record.name
        && artifact.key == key
        && artifact_target_matches_host(artifact.target)
        && !artifact.fingerprint.is_empty()
        && artifact.abi == record.abi
        && artifact.entry == record.entry
        && artifact.external_entries == record.external_entries
        && artifact.has_fallthrough == record.fallthrough.is_some()
        && (artifact.has_fallthrough == fallthrough.is_some())
        && artifact.fallthrough.is_none_or(|_| record.fallthrough.is_some())
        && record.fallthrough.is_none_or(|(_, entry)| {
            artifact.fallthrough_entry == entry
        });
    let effects_match = artifact.executable == record.executable
        && artifact.template_calls_helper == record.template_calls_helper;
    if !metadata_matches
        || !effects_match
        || !artifact.stencil.validate()
        || !artifact.fallthrough.is_none_or(|stencil| stencil.validate())
    {
        return None;
    }
    Some(PhysicalStencilView {
        key,
        record,
        stencil: &artifact.stencil,
        generated: true,
        abi: artifact.abi,
        entry: artifact.entry,
        external_entries: artifact.external_entries,
        fallthrough,
        executable: artifact.executable,
        template_calls_helper: artifact.template_calls_helper,
        target: Some(artifact.target),
        fingerprint: Some(artifact.fingerprint),
    })
}

fn artifact_target_matches_host(target: &str) -> bool {
    let exact_target = option_env!("QUENCH_BUILD_TARGET").is_some_and(|expected| expected == target);
    if !exact_target {
        return false;
    }
    #[cfg(target_arch = "aarch64")]
    return target.starts_with("aarch64");
    #[cfg(target_arch = "x86_64")]
    return target.starts_with("x86_64");
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = target;
        false
    }
}

pub fn select_region(key: RegionKey) -> Option<&'static RegionRecord> {
    canonical_region_lookup(key)
}

/// Iterate the build-time declaration table for plan construction.  Keeping
/// this accessor next to selection means callers cannot drift a second,
/// hand-maintained list of region keys from the generated catalog.
pub(crate) fn region_records() -> &'static [RegionRecord] {
    CANONICAL_REGION_TABLE
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
    CANONICAL_REGION_TABLE.len()
}

#[cfg(test)]
mod generated_region_admission_tests {
    use super::*;

    #[test]
    fn every_generated_region_has_one_external_entry_and_exact_ops() {
        for record in CANONICAL_REGION_TABLE {
            assert_eq!(record.entry, 0);
            assert_eq!(
                select_region(record.key).unwrap().operations,
                record.operations
            );
            assert!(has_single_entry_point(
                u32::from(record.entry),
                &[RegionBlock {
                    id: 0,
                    predecessors: &[],
                    external_entry: true
                }]
            ));
        }
        assert!(!has_single_entry_point(
            0,
            &[
                RegionBlock {
                    id: 0,
                    predecessors: &[],
                    external_entry: true
                },
                RegionBlock {
                    id: 1,
                    predecessors: &[0],
                    external_entry: true
                },
            ]
        ));
    }

    #[test]
    fn generated_abi_classification_matches_physical_entry_shape() {
        for record in CANONICAL_REGION_TABLE {
            match record.abi {
                RegionAbi::Scalar => {
                    assert!(!record.stencil.bytes.is_empty());
                    assert_ne!(record.stencil.bytes.len(), 44);
                    assert_ne!(record.stencil.bytes.len(), 76);
                }
                RegionAbi::TaggedWord => {
                    assert!(matches!(record.stencil.bytes.len(), 4 | 8));
                    assert!(matches!(
                        record.operations.first(),
                        Some(
                            crate::ir::Opcode::Move
                                | crate::ir::Opcode::LoadLocal
                                | crate::ir::Opcode::StoreLocal
                                | crate::ir::Opcode::GetN
                                | crate::ir::Opcode::SetN,
                        )
                    ));
                }
                RegionAbi::ConstantWord => {
                    assert!(matches!(record.stencil.bytes.len(), 11 | 16));
                    assert!(matches!(
                        record.operations,
                        [crate::ir::Opcode::LoadConst, crate::ir::Opcode::Return]
                    ));
                }
                RegionAbi::ScalarBool => {
                    if matches!(record.operations, [crate::ir::Opcode::JumpIfFalse]) {
                        assert!(matches!(record.stencil.bytes.len(), 23 | 28));
                    } else {
                        assert!(matches!(
                            record.operations,
                            [crate::ir::Opcode::Binary, crate::ir::Opcode::Return]
                        ));
                        assert!(matches!(record.stencil.bytes.len(), 11 | 12 | 16 | 20));
                    }
                }
                RegionAbi::ScalarWordBool => {
                    assert!(matches!(
                        record.stencil.bytes.len(),
                        6 | 8 | 20 | 24 | 27 | 32
                    ));
                    assert!(matches!(
                        record.operations,
                        [crate::ir::Opcode::Unary, crate::ir::Opcode::Return]
                            | [crate::ir::Opcode::JumpIfFalse]
                    ));
                }
                RegionAbi::ScalarWordPairBool => {
                    assert!(matches!(record.stencil.bytes.len(), 10 | 12));
                    assert!(matches!(
                        record.operations,
                        [crate::ir::Opcode::Binary, crate::ir::Opcode::Return]
                    ));
                }
                RegionAbi::ScalarI32 => {
                    assert!(matches!(record.stencil.bytes.len(), 5 | 8));
                    assert!(matches!(
                        record.operations,
                        [crate::ir::Opcode::Binary, crate::ir::Opcode::Return]
                            | [crate::ir::Opcode::Unary, crate::ir::Opcode::Return]
                    ));
                }
                RegionAbi::ScalarU32 => {
                    assert!(matches!(record.stencil.bytes.len(), 7 | 8));
                    assert!(record
                        .operations
                        .starts_with(&[crate::ir::Opcode::Binary, crate::ir::Opcode::Return]));
                }
                RegionAbi::Bridge => {
                    assert!(
                        matches!(record.stencil.bytes.len(), 12 | 16),
                        "bridge rows use the dispatch trampoline"
                    );
                }
                RegionAbi::ArrayKernel => {
                    assert!(matches!(record.stencil.bytes.len(), 12 | 20 | 32 | 44))
                }
                RegionAbi::ArrayNumericLoop => assert_eq!(record.stencil.bytes.len(), 100),
            }
        }
    }

    #[test]
    fn raw_array_rows_advertise_execution_only_for_their_emitter_target() {
        for record in CANONICAL_REGION_TABLE {
            if matches!(record.abi, RegionAbi::ArrayKernel) {
                assert_eq!(
                    record.executable,
                    cfg!(target_arch = "aarch64"),
                    "raw array ABI must not route trampoline bytes as a kernel"
                );
            }
        }
    }

    #[test]
    fn generated_contracts_reuse_opcode_effects_and_entry_rules() {
        let scalar = select_region(loop_region_key())
            .expect("scalar row")
            .contract();
        assert_eq!(scalar.abi, RegionAbi::Scalar);
        assert!(scalar.has_effect(crate::facts::OperationEffect::MayThrow));
        assert!(!scalar.has_effect(crate::facts::OperationEffect::WriteHeap));
        assert!(scalar.legal_external_entry(0));
        assert!(!scalar.legal_external_entry(1));

        let array = select_region(array_numeric_loop_region_key())
            .expect("numeric loop row")
            .contract();
        assert_eq!(array.abi, RegionAbi::ArrayNumericLoop);
        assert!(array.has_effect(crate::facts::OperationEffect::ReadHeap));
        assert!(array.has_effect(crate::facts::OperationEffect::WriteHeap));
        assert!(array.has_effect(crate::facts::OperationEffect::Control));
        assert!(array.requires_semantic_boundary());
        assert!(array.has_single_entry());
        assert_eq!(
            select_region(property_region_key())
                .expect("property row")
                .abi,
            RegionAbi::TaggedWord
        );
        assert_eq!(
            select_region(move_region_key()).expect("move row").abi,
            RegionAbi::TaggedWord
        );
    }

    #[test]
    fn abi_contracts_keep_scalar_bridge_and_raw_entries_distinct() {
        assert_eq!(RegionAbi::Scalar.contract().context_arg_words, 0);
        assert!(RegionAbi::Scalar.contract().preserves_vm_registers);
        assert_eq!(RegionAbi::TaggedWord.contract().context_arg_words, 0);
        assert!(RegionAbi::TaggedWord.contract().preserves_vm_registers);
        assert_eq!(RegionAbi::ScalarI32.contract().context_arg_words, 0);
        assert!(RegionAbi::ScalarI32.contract().preserves_vm_registers);
        assert_eq!(RegionAbi::ScalarU32.contract().context_arg_words, 0);
        assert!(RegionAbi::ScalarU32.contract().preserves_vm_registers);
        assert!(RegionAbi::Bridge.contract().may_call_helper);
        assert_eq!(RegionAbi::ArrayKernel.contract().context_arg_words, 1);
        assert!(!RegionAbi::ArrayKernel.contract().may_call_helper);
        assert!(!RegionAbi::Bridge.contract().interruptible_backedge);
        assert!(
            RegionAbi::ArrayNumericLoop
                .contract()
                .interruptible_backedge
        );
        for record in CANONICAL_REGION_TABLE {
            assert!(record.contract().abi_is_well_formed());
            assert_eq!(
                record.abi.accepts_region_context(),
                record.contract().abi_contract().context_arg_words == 1
            );
        }
        assert_eq!(RegionAbi::Scalar.contract().hardware_clobber_mask, 0);
        assert_eq!(RegionAbi::ArrayNumericLoop.contract().live_out_mask, 0x0003);
        assert!(RegionAbi::Bridge.contract().root_materialization_required);
    }
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
        assert!(select_stencil(loop_region_key()).is_some());
        assert!(select_stencil(RegionKey(0)).is_none());
        assert_eq!(
            loop_region_key(),
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

    #[test]
    fn numeric_add_leaf_never_selects_nonreturning_fallthrough_head() {
        let key = numeric_region_key(crate::ir::Opcode::Add).expect("add leaf");
        let record = select_region(key).expect("add declaration");
        assert_eq!(key, loop_region_key());
        assert!(record.fallthrough.is_none());
        assert!(fallthrough_region_key() != key);
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
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
        let key = property_region_key();
        let record = select_region(key).expect("property admission row");
        assert_eq!(
            record.executable,
            cfg!(any(target_arch = "x86_64", target_arch = "aarch64"))
        );
        assert_eq!(
            record.stencil.bytes.len(),
            if cfg!(target_arch = "x86_64") {
                4
            } else if cfg!(target_arch = "aarch64") {
                8
            } else {
                1
            }
        );
        assert!(record.stencil.holes.is_empty());
    }

    #[test]
    fn move_row_is_catalog_admitted() {
        let key = move_region_key();
        let record = select_region(key).expect("move admission row");
        assert_eq!(
            record.executable,
            cfg!(any(target_arch = "x86_64", target_arch = "aarch64"))
        );
        assert_eq!(
            record.stencil.bytes.len(),
            if cfg!(target_arch = "x86_64") {
                4
            } else if cfg!(target_arch = "aarch64") {
                8
            } else {
                1
            }
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
            if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
                1
            } else {
                0
            }
        );
    }

    #[test]
    fn quickened_catalog_entries_use_the_same_cfg_checked_dispatch_region() {
        let record = select_region(dispatch_region_key()).expect("dispatch admission row");
        for opcode in [
            crate::ir::Opcode::GetPropertyQuickened,
            crate::ir::Opcode::GetNQuickened,
            crate::ir::Opcode::AGetIQuickened,
        ] {
            assert!(record.operations.contains(&opcode));
        }
    }

    #[test]
    fn generated_accessor_matches_legacy_fallthrough_key() {
        // Explicit before/after migration check: the former hand-written
        // construction and the generated declaration are identical.
        let legacy = RegionKey::from_opcodes(
            RegionId(4),
            &[crate::ir::Opcode::Add, crate::ir::Opcode::Return],
        );
        assert_eq!(fallthrough_region_key(), legacy);
    }

    #[test]
    fn dispatch_uses_region_sequence_and_falls_back_once() {
        let selected = dispatch_region(
            loop_region_key(),
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
    fn cfg_rejects_external_entry_into_multi_instruction_span_interior() {
        // A fused span must contain at least three operations for this check:
        // an entry at the final operation is still an externally reachable
        // interior entry and therefore cannot be rendered as one atomic
        // single-entry region.
        let blocks = [
            RegionBlock {
                id: 0,
                predecessors: &[],
                external_entry: true,
            },
            RegionBlock {
                id: 1,
                predecessors: &[0],
                external_entry: false,
            },
            RegionBlock {
                id: 2,
                predecessors: &[1, 9],
                external_entry: true,
            },
        ];
        assert!(!has_single_entry_point(0, &blocks));
    }

    #[test]
    fn loop_body_span_is_single_entry_and_rejects_interior_edges() {
        let record = select_region(loop_body_region_key()).expect("loop body row");
        assert_eq!(record.operations.len(), 7);
        assert!(has_single_entry_point(
            u32::from(record.entry),
            &[RegionBlock {
                id: 0,
                predecessors: &[],
                external_entry: true,
            }]
        ));
        assert!(!has_single_entry_point(
            0,
            &[
                RegionBlock {
                    id: 0,
                    predecessors: &[],
                    external_entry: true,
                },
                RegionBlock {
                    id: 3,
                    predecessors: &[2],
                    external_entry: true,
                },
            ]
        ));
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

    #[test]
    fn extracted_build_artifacts_match_canonical_bytes() {
        #[cfg(quench_generated_stencil_artifacts)]
        assert!(
            !BUILD_STENCIL_ARTIFACTS.is_empty(),
            "enabled Rust extraction must publish at least one artifact"
        );
        for artifact in BUILD_STENCIL_ARTIFACTS {
            let record = CANONICAL_REGION_TABLE
                .iter()
                .find(|record| record.name == artifact.name)
                .expect("artifact declaration has a catalog row");
            if record.stencil.holes.is_empty() && artifact.name != "array_numeric_loop" {
                assert_eq!(
                    artifact.bytes, record.stencil.bytes,
                    "artifact {} drifted",
                    artifact.name
                );
            } else if artifact.has_fallthrough || artifact.name == "array_numeric_loop" {
                assert!(!artifact.bytes.is_empty());
                if artifact.has_fallthrough {
                    assert!(!artifact.stencil.holes.is_empty());
                    assert!(artifact.fallthrough.is_some());
                } else {
                    assert!(artifact.stencil.holes.is_empty());
                }
            } else {
                assert!(
                    !artifact.bytes.is_empty() && artifact.stencil.holes.is_empty(),
                    "hole-bearing {} requires a complete generated whole-function recipe",
                    artifact.name
                );
            }
            assert!(!artifact.fingerprint.is_empty());
            assert!(!artifact.target.is_empty());
            assert_eq!(artifact.abi, record.abi);
            assert_eq!(artifact.key, record.key);
        }
        if !BUILD_STENCIL_ARTIFACTS.is_empty() {
            let chain = BUILD_STENCIL_ARTIFACTS
                .iter()
                .find(|artifact| artifact.key == add_chain_region_key())
                .expect("Rust generation must include the fused arithmetic chain");
            let chain_record = CANONICAL_REGION_TABLE
                .iter()
                .find(|record| record.name == "add_chain")
                .expect("fused chain declaration");
            assert_eq!(chain.bytes, chain_record.stencil.bytes);
            assert_eq!(
                select_stencil(chain_record.key).map(|stencil| stencil.bytes),
                Some(chain.bytes),
                "normal selection must use the generated chain artifact"
            );
        }
    }

    #[test]
    fn physical_view_rejects_layout_mismatch_before_entry() {
        static BYTES: &[u8] = &[0xC3];
        static WRONG_ENTRIES: &[u16] = &[1];
        const TARGET: &str = match option_env!("QUENCH_BUILD_TARGET") {
            Some(target) => target,
            None => "test",
        };
        static BAD_ENTRY: BuildStencilArtifact = BuildStencilArtifact {
            name: "add_const",
            key: RegionKey(0),
            target: "test",
            compiler: "test",
            fingerprint: "test",
            abi: RegionAbi::Scalar,
            entry: 1,
            external_entries: &[0],
            has_fallthrough: false,
            executable: true,
            template_calls_helper: false,
            bytes: BYTES,
            stencil: Stencil { bytes: BYTES, holes: &[] },
            fallthrough: None,
            fallthrough_entry: 0,
        };
        static BAD_ENTRIES: BuildStencilArtifact = BuildStencilArtifact {
            name: "add_const",
            key: RegionKey(0),
            target: "test",
            compiler: "test",
            fingerprint: "test",
            abi: RegionAbi::Scalar,
            entry: 0,
            external_entries: WRONG_ENTRIES,
            has_fallthrough: false,
            executable: true,
            template_calls_helper: false,
            bytes: BYTES,
            stencil: Stencil { bytes: BYTES, holes: &[] },
            fallthrough: None,
            fallthrough_entry: 0,
        };
        static BAD_LAYOUT: BuildStencilArtifact = BuildStencilArtifact {
            name: "add_const",
            key: RegionKey(0),
            target: TARGET,
            compiler: "test",
            fingerprint: "test",
            abi: RegionAbi::Scalar,
            entry: 0,
            external_entries: &[0],
            has_fallthrough: true,
            executable: true,
            template_calls_helper: false,
            bytes: BYTES,
            stencil: Stencil { bytes: BYTES, holes: &[] },
            fallthrough: None,
            fallthrough_entry: 9,
        };
        let record = CANONICAL_REGION_TABLE
            .iter()
            .find(|record| record.name == "add_const")
            .expect("add_const row");
        assert!(generated_physical_view(record.key, record, &BAD_ENTRY).is_none());
        assert!(generated_physical_view(record.key, record, &BAD_ENTRIES).is_none());
        assert!(generated_physical_view(record.key, record, &BAD_LAYOUT).is_none());

        static BAD_TARGET: BuildStencilArtifact = BuildStencilArtifact {
            name: "add_const",
            key: RegionKey(0),
            target: "mismatched-target",
            compiler: "test",
            fingerprint: "test",
            abi: RegionAbi::Scalar,
            entry: 0,
            external_entries: &[0],
            has_fallthrough: false,
            executable: true,
            template_calls_helper: false,
            bytes: BYTES,
            stencil: Stencil { bytes: BYTES, holes: &[] },
            fallthrough: None,
            fallthrough_entry: 0,
        };
        assert!(generated_physical_view(record.key, record, &BAD_TARGET).is_none());
    }
}
